use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use core::convert::Infallible;
use hashbrown::HashMap;
use log::warn;
use rustix::io::Errno;
use rustix::process::{Pid, chdir, chroot};
use thiserror::Error;
use typed_path::{UnixPath, UnixPathBuf};

use crate::env;
use crate::fsutil;
use crate::procfs;
use crate::spawn;

#[derive(Debug, Error)]
pub(crate) enum CmdError {
    #[error("failed to read environment file {}", path.display())]
    OpenEnviron {
        path: UnixPathBuf,
        #[source]
        source: Errno,
    },
    #[error("failed to read container root from {path}")]
    ReadContainerRoot {
        path: String,
        #[source]
        source: Errno,
    },
    #[error("failed to execute command: {command}")]
    Exec {
        command: String,
        #[source]
        source: Errno,
    },
}

pub(crate) struct Cmd {
    environment: HashMap<Vec<u8>, Vec<u8>>,
    command: String,
    arguments: Vec<String>,
    home: Option<UnixPathBuf>,
    container_root: UnixPathBuf,
}

fn read_environment(pid: Pid) -> Result<HashMap<Vec<u8>, Vec<u8>>, CmdError> {
    let path = procfs::get_path().join(pid.to_string()).join("environ");
    let contents = fsutil::read(&path).map_err(|source| CmdError::OpenEnviron { path, source })?;
    let res: HashMap<Vec<u8>, Vec<u8>> = contents
        .split(|b| *b == b'\0')
        .filter_map(|var| {
            let tuple: Vec<&[u8]> = var.splitn(2, |b| *b == b'=').collect();
            if tuple.len() != 2 {
                return None;
            }
            Some((Vec::from(tuple[0]), Vec::from(tuple[1])))
        })
        .collect();
    Ok(res)
}

/// Try to read PATH from container's /etc/environment
///
/// Attempts to extract PATH from /etc/environment under the container root.
/// Returns None if the file cannot be read or PATH is not found.
fn read_container_path(container_root: &UnixPath) -> Option<Vec<u8>> {
    let etc_environment = container_root.join("etc/environment");
    let contents = fsutil::read_to_string(&etc_environment).ok()?;

    for line in contents.lines() {
        let trimmed = line.trim();
        // Look for PATH=... or PATH="..."
        if let Some(path_value) = trimmed.strip_prefix("PATH=") {
            let path_value = path_value.trim_matches('"').trim_matches('\'');
            if !path_value.is_empty() {
                return Some(path_value.as_bytes().to_vec());
            }
        }
    }

    None
}

impl Cmd {
    pub(crate) fn new(
        command: Option<String>,
        args: Vec<String>,
        pid: Pid,
        home: Option<UnixPathBuf>,
    ) -> Result<Cmd, CmdError> {
        let arguments = if command.is_none() {
            vec![String::from("-l")]
        } else {
            args
        };

        let command = command.unwrap_or_else(|| env::var("SHELL").unwrap_or("sh").to_string());

        let variables = read_environment(pid)?;

        // Read container root path before entering namespaces
        // After entering PID namespace, /proc/{container_pid} won't be accessible
        let proc_root_path = format!("/proc/{}/root", pid);
        let container_root =
            fsutil::read_link(&proc_root_path).map_err(|source| CmdError::ReadContainerRoot {
                path: proc_root_path,
                source,
            })?;

        Ok(Cmd {
            command,
            arguments,
            environment: variables,
            home,
            container_root,
        })
    }

    /// Execute in attach mode - no chroot, uses overlay
    ///
    /// For attach, we stay in the overlay environment which provides access
    /// to both host binaries and container filesystem under /var/lib/cntr
    ///
    /// This function never returns on success - it replaces the current process.
    pub(crate) fn exec_in_overlay(mut self) -> Result<Infallible, CmdError> {
        // Set PATH if not already set (use cntr's PATH or default)
        let default_path = "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";
        self.environment.insert(
            b"PATH".to_vec(),
            env::var("PATH").unwrap_or(default_path).into(),
        );

        // Set HOME if effective user was specified
        if let Some(home_path) = self.home {
            self.environment
                .insert(b"HOME".to_vec(), home_path.into_vec());
        }

        // Execute without chroot - we're already in the overlay
        let err = spawn::exec(&self.command, &self.arguments, &self.environment)
            .expect_err("exec only returns on error");
        Err(CmdError::Exec {
            command: self.command,
            source: err,
        })
    }

    /// Execute in container - chroot to container root
    ///
    /// For exec (direct mode) and daemon exec, we chroot to the actual container
    /// root since we don't have the overlay.
    ///
    /// This function never returns on success - it replaces the current process.
    pub(crate) fn exec_in_container(
        mut self,
        lsm_profile: Option<(UnixPathBuf, String)>,
    ) -> Result<Infallible, CmdError> {
        // Set PATH only if not already present in container environment
        // Avoid using host's PATH which may point to binaries not present after chroot
        if !self.environment.contains_key(b"PATH".as_slice()) {
            let default_path =
                b"/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin".to_vec();
            let path = read_container_path(&self.container_root).unwrap_or(default_path);
            self.environment.insert(b"PATH".to_vec(), path);
        }

        // Chroot to container's root and exec. We are already the process
        // that will become the container command, so no extra fork is needed.
        // container_root was already resolved in new() before entering namespaces
        let container_root = self.container_root;
        let err = (|| -> Errno {
            if let Err(e) = chroot(container_root.as_bytes()) {
                warn!("failed to chroot to {}: {}", container_root.display(), e);
                return e;
            }

            if let Err(e) = chdir("/") {
                warn!("failed to change directory to /");
                return e;
            }

            // Apply AppArmor profile after chroot
            if let Some((path, label)) = &lsm_profile {
                let attr = format!("changeprofile {}", label);
                if let Err(e) = fsutil::write_existing(path, attr) {
                    return e;
                }
            }

            spawn::exec(&self.command, &self.arguments, &self.environment)
                .expect_err("exec only returns on error")
        })();
        Err(CmdError::Exec {
            command: self.command,
            source: err,
        })
    }
}
