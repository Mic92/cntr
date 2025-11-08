use anyhow::{Context, bail};
use nix::sys::wait::{WaitStatus, waitpid};
use nix::unistd::{self, ForkResult};
use std::os::fd::{AsRawFd, IntoRawFd};
use std::process;

use crate::cmd::Cmd;
use crate::container::ContainerContext;
use crate::container_setup;
use crate::pty;
use crate::result::Result;
use crate::syscalls::capability;

/// Execute a command in a container
///
/// Directly accesses container by ID/name with PTY.
///
/// Arguments:
/// - container_name: Container ID, name, or PID
/// - container_types: List of container types to try
/// - exe: Optional command to execute (None = default shell)
/// - args: Arguments to pass to the command
pub(crate) fn exec(
    container_name: &str,
    container_types: &[Box<dyn container_pid::Container>],
    exe: Option<String>,
    args: Vec<String>,
) -> Result<()> {
    // Verify mount API capability
    if !capability::has_mount_api() {
        bail!(
            "Linux mount API is not available. cntr requires kernel 6.8+ with mount API support.\n\
             Please upgrade your kernel or use an older version of cntr with FUSE support."
        );
    }

    // Lookup container and get its context
    let ctx = ContainerContext::lookup(container_name, container_types)?;

    // Create PTY for interactive command execution
    let pty_master = pty::open_ptm().context("failed to open pty master")?;

    // Fork: child enters container and execs, parent forwards PTY I/O
    let res = unsafe { unistd::fork() };
    match res.context("failed to fork")? {
        ForkResult::Parent { child } => {
            // Parent: Forward PTY I/O and wait for child
            exec_parent(child, &pty_master)
        }
        ForkResult::Child => {
            // Child: Setup PTY slave, enter container, exec command
            if let Err(e) = exec_child(&ctx, exe, args, &pty_master) {
                eprintln!("exec child failed: {}", e);
                process::exit(1);
            }
            // Should not reach here - exec_child calls process::exit
            unreachable!()
        }
    }
}

/// Parent process for exec: Forward PTY and wait for child
fn exec_parent(child_pid: nix::unistd::Pid, pty_master: &nix::pty::PtyMaster) -> Result<()> {
    // Close master PTY fd in child before forwarding
    // (child has slave end)
    let pty_file = unsafe {
        use std::fs::File;
        use std::os::fd::FromRawFd;
        File::from_raw_fd(pty_master.as_raw_fd())
    };

    // Forward PTY I/O
    // This will block until child exits or PTY closes
    let _ = pty::forward(&pty_file);

    // Don't close the PTY file (avoid double-free)
    let _ = pty_file.into_raw_fd();

    // Wait for child to exit
    match waitpid(child_pid, None) {
        Ok(WaitStatus::Exited(_, status)) => {
            process::exit(status);
        }
        Ok(WaitStatus::Signaled(_, signal, _)) => {
            // Child was signaled - send same signal to ourselves
            nix::sys::signal::kill(unistd::getpid(), signal)
                .with_context(|| format!("failed to send signal {:?} to own process", signal))?;
        }
        Ok(status) => {
            bail!("child exited with unexpected status: {:?}", status);
        }
        Err(e) => {
            Err(e).context("failed to wait for child")?;
        }
    }

    Ok(())
}

/// Child process for exec: Enter container and exec command
fn exec_child(
    ctx: &ContainerContext,
    exe: Option<String>,
    args: Vec<String>,
    pty_master: &nix::pty::PtyMaster,
) -> Result<()> {
    // Attach PTY slave
    pty::attach_pts(pty_master).context("failed to setup pty slave")?;

    // Default to /bin/sh if no command specified
    let exe = exe.or(Some(String::from("/bin/sh")));

    // Prepare command to execute
    let cmd = Cmd::new(exe, args, ctx.process_status.global_pid, None)?;

    // Enter container: cgroup, namespaces, security context (LSM, UID/GID, capabilities)
    container_setup::enter_container(ctx.process_status.global_pid, &ctx.process_status)?;

    // Execute the command in the container (chroots to container root and execs)
    // This will NOT return - it replaces the current process
    cmd.exec_in_container()
        .context("failed to execute command in container")?;

    Ok(())
}
