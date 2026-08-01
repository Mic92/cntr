use log::warn;
use rustix::process::{Pid, chroot};
use std::collections::HashMap;
use std::convert::Infallible;
use std::env;
use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::OsStringExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use thiserror::Error;

use crate::fsutil;
use crate::procfs;

#[derive(Debug, Error)]
pub(crate) enum CmdError {
    #[error("failed to read environment file {path}")]
    OpenEnviron {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to read container root from {path}")]
    ReadContainerRoot {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to execute command: {command}")]
    Exec {
        command: String,
        #[source]
        source: std::io::Error,
    },
}

pub(crate) struct Cmd {
    environment: HashMap<OsString, OsString>,
    command: String,
    arguments: Vec<String>,
    home: Option<PathBuf>,
    container_root: PathBuf,
}

fn read_environment(pid: Pid) -> Result<HashMap<OsString, OsString>, CmdError> {
    let path = procfs::get_path().join(pid.to_string()).join("environ");
    let contents = fsutil::read(&path).map_err(|source| CmdError::OpenEnviron { path, source })?;
    let res: HashMap<OsString, OsString> = contents
        .split(|b| *b == b'\0')
        .filter_map(|var| {
            let tuple: Vec<&[u8]> = var.splitn(2, |b| *b == b'=').collect();
            if tuple.len() != 2 {
                return None;
            }
            Some((
                OsString::from_vec(Vec::from(tuple[0])),
                OsString::from_vec(Vec::from(tuple[1])),
            ))
        })
        .collect();
    Ok(res)
}

/// Try to read PATH from container's /etc/environment
///
/// Attempts to extract PATH from /etc/environment under the container root.
/// Returns None if the file cannot be read or PATH is not found.
fn read_container_path(container_root: &Path) -> Option<OsString> {
    let etc_environment = container_root.join("etc/environment");
    let contents = fsutil::read_to_string(&etc_environment).ok()?;

    for line in contents.lines() {
        let trimmed = line.trim();
        // Look for PATH=... or PATH="..."
        if let Some(path_value) = trimmed.strip_prefix("PATH=") {
            let path_value = path_value.trim_matches('"').trim_matches('\'');
            if !path_value.is_empty() {
                return Some(OsString::from(path_value));
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
        home: Option<PathBuf>,
    ) -> Result<Cmd, CmdError> {
        let arguments = if command.is_none() {
            vec![String::from("-l")]
        } else {
            args
        };

        let command =
            command.unwrap_or_else(|| env::var("SHELL").unwrap_or_else(|_| String::from("sh")));

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
        let default_path =
            OsString::from("/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin");
        self.environment.insert(
            OsString::from("PATH"),
            env::var_os("PATH").unwrap_or(default_path),
        );

        // Set HOME if effective user was specified
        if let Some(home_path) = self.home {
            self.environment
                .insert(OsString::from("HOME"), home_path.into_os_string());
        }

        // Execute without chroot - we're already in the overlay
        let err = Command::new(&self.command)
            .args(self.arguments)
            .envs(self.environment)
            .exec();
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
        lsm_profile: Option<(PathBuf, String)>,
    ) -> Result<Infallible, CmdError> {
        // Set PATH only if not already present in container environment
        // Avoid using host's PATH which may point to binaries not present after chroot
        if !self.environment.contains_key(OsStr::new("PATH")) {
            let default_path =
                OsString::from("/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin");
            let path = read_container_path(&self.container_root).unwrap_or(default_path);
            self.environment.insert(OsString::from("PATH"), path);
        }

        // Chroot to container's root and exec
        // container_root was already resolved in new() before entering namespaces
        let container_root = self.container_root;
        let err = unsafe {
            Command::new(&self.command)
                .args(self.arguments)
                .envs(self.environment)
                .pre_exec(move || {
                    // First do chroot
                    if let Err(e) = chroot(&container_root) {
                        warn!("failed to chroot to {}: {}", container_root.display(), e);
                        return Err(e.into());
                    }

                    if let Err(e) = env::set_current_dir("/") {
                        warn!("failed to change directory to /");
                        return Err(e);
                    }

                    // Apply AppArmor profile after chroot
                    if let Some((path, label)) = &lsm_profile {
                        let attr = format!("changeprofile {}", label);
                        crate::fsutil::write_existing(path, attr)?;
                    }

                    Ok(())
                })
                .exec()
        };
        Err(CmdError::Exec {
            command: self.command,
            source: err,
        })
    }
}
