

use crate::syscalls::process::{Fork, exit_group, fork};
use rustix::io::Errno;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::convert::Infallible;
use rustix::fd::OwnedFd;
use thiserror::Error;

use crate::ApparmorMode;
use crate::cmd::{Cmd, CmdError};
use crate::container::ContainerError;
use crate::container_setup::{self, SetupError};
use crate::errors::format_chain;
use crate::pty::{self, PtyError};
use crate::syscalls::capability;

#[derive(Debug, Error)]
pub(crate) enum ExecError {
    #[error(
        "Linux mount API is not available. cntr requires kernel 6.8+ with mount API support.\n\
         Please upgrade your kernel or use an older version of cntr with FUSE support."
    )]
    MountApiUnavailable,
    #[error("failed to lookup container '{container}'")]
    LookupContainer {
        container: String,
        #[source]
        source: ContainerError,
    },
    #[error("failed to fork")]
    Fork(#[source] Errno),
    #[error(transparent)]
    Pty(#[from] PtyError),
    #[error(transparent)]
    Cmd(#[from] CmdError),
    #[error("failed to enter container")]
    Setup(#[from] SetupError),
}

pub(crate) struct ExecOptions {
    pub(crate) command: Option<String>,
    pub(crate) arguments: Vec<String>,
    pub(crate) container_name: String,
    pub(crate) container_types: Vec<Box<dyn crate::container_pid::Container>>,
    pub(crate) apparmor_mode: ApparmorMode,
}

/// Execute a command in a container
///
/// Directly accesses container by ID/name with PTY.
pub(crate) fn exec(opts: &ExecOptions) -> Result<Infallible, ExecError> {
    // Verify mount API capability
    if !capability::has_mount_api() {
        return Err(ExecError::MountApiUnavailable);
    }

    // Lookup container and get its process status
    let mut process_status = crate::container::lookup_container(
        &opts.container_name,
        &opts.container_types,
        opts.apparmor_mode,
    )
    .map_err(|source| ExecError::LookupContainer {
        container: opts.container_name.clone(),
        source,
    })?;

    // Create PTY for interactive command execution
    let pty_master = pty::open_ptm()?;

    // Fork: child enters container and execs, parent forwards PTY I/O
    match fork().map_err(ExecError::Fork)? {
        Fork::ParentOf(child) => {
            // Parent: Forward PTY I/O and wait for child
            Ok(pty::forward_pty_and_wait(&pty_master, child)?)
        }
        Fork::Child => {
            // Child: Setup PTY slave, enter container, exec command
            let Err(e) = exec_child(
                &mut process_status,
                opts.command.clone(),
                opts.arguments.clone(),
                &pty_master,
            );
            crate::stderrln!("exec child failed: {}", format_chain(&e));
            exit_group(1);
        }
    }
}

/// Child process for exec: Enter container and exec command
///
/// This function never returns on success - it replaces the current process.
fn exec_child(
    process_status: &mut crate::procfs::ProcStatus,
    exe: Option<String>,
    args: Vec<String>,
    pty_master: &OwnedFd,
) -> Result<Infallible, ExecError> {
    // Attach PTY slave
    pty::attach_pts(pty_master)?;

    // Default to /bin/sh if no command specified
    let exe = exe.or(Some(String::from("/bin/sh")));

    // Prepare command to execute
    let cmd = Cmd::new(exe.clone(), args, process_status.global_pid, None)?;

    // Enter container: cgroup, namespaces, security context (UID/GID, capabilities)
    // Note: AppArmor is NOT applied yet - we do it in pre_exec after chroot
    container_setup::enter_container(process_status)?;

    // Extract LSM profile info for pre_exec hook
    let lsm_profile = process_status
        .lsm_profile
        .as_ref()
        .map(|p| (p.own_path.clone(), p.label.clone()));

    // Execute the command in the container (chroots to container root and execs)
    // AppArmor will be applied in pre_exec after chroot
    // This will NOT return on success - it replaces the current process
    Ok(cmd.exec_in_container(lsm_profile)?)
}
