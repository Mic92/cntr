use anyhow::{Context, bail};
use nix::unistd::{self, ForkResult};
use std::process;

use crate::ApparmorMode;
use crate::cmd::Cmd;
use crate::container_setup;
use crate::pty;
use crate::result::Result;
use crate::syscalls::capability;

pub(crate) struct ExecOptions {
    pub(crate) command: Option<String>,
    pub(crate) arguments: Vec<String>,
    pub(crate) container_name: String,
    pub(crate) container_types: Vec<Box<dyn container_pid::Container>>,
    pub(crate) apparmor_mode: ApparmorMode,
}

/// Execute a command in a container
///
/// Directly accesses container by ID/name with PTY.
pub(crate) fn exec(opts: &ExecOptions) -> Result<std::convert::Infallible> {
    // Verify mount API capability
    if !capability::has_mount_api() {
        bail!(
            "Linux mount API is not available. cntr requires kernel 6.8+ with mount API support.\n\
             Please upgrade your kernel or use an older version of cntr with FUSE support."
        );
    }

    // Lookup container and get its process status
    let mut process_status = crate::container::lookup_container(
        &opts.container_name,
        &opts.container_types,
        opts.apparmor_mode,
    )
    .with_context(|| format!("failed to lookup container '{}'", opts.container_name))?;

    // Create PTY for interactive command execution
    let pty_master = pty::open_ptm().context("failed to open pty master")?;

    // Fork: child enters container and execs, parent forwards PTY I/O
    let res = unsafe { unistd::fork() };
    match res.context("failed to fork")? {
        ForkResult::Parent { child } => {
            // Parent: Forward PTY I/O and wait for child
            pty::forward_pty_and_wait(&pty_master, child)
        }
        ForkResult::Child => {
            // Child: Setup PTY slave, enter container, exec command
            let Err(e) = exec_child(
                &mut process_status,
                opts.command.clone(),
                opts.arguments.clone(),
                &pty_master,
            );
            eprintln!("exec child failed: {:?}", e);
            process::exit(1);
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
    pty_master: &nix::pty::PtyMaster,
) -> Result<std::convert::Infallible> {
    // Attach PTY slave
    pty::attach_pts(pty_master).context("failed to setup pty slave")?;

    // Default to /bin/sh if no command specified
    let exe = exe.or(Some(String::from("/bin/sh")));

    // Prepare command to execute
    let cmd = Cmd::new(exe.clone(), args, process_status.global_pid, None)
        .with_context(|| format!("failed to prepare command {:?}", exe))?;

    // Enter container: cgroup, namespaces, security context (UID/GID, capabilities)
    // Note: AppArmor is NOT applied yet - we do it in pre_exec after chroot
    container_setup::enter_container(process_status).with_context(|| {
        format!(
            "failed to enter container with PID {}",
            process_status.global_pid
        )
    })?;

    // Extract LSM profile info for pre_exec hook
    let lsm_profile = process_status
        .lsm_profile
        .as_ref()
        .map(|p| (p.own_path.clone(), p.label.clone()));

    // Execute the command in the container (chroots to container root and execs)
    // AppArmor will be applied in pre_exec after chroot
    // This will NOT return on success - it replaces the current process
    cmd.exec_in_container(lsm_profile)
        .context("failed to execute command in container")
}
