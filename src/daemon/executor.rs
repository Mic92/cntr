use anyhow::Context;
use log::warn;
use nix::sys::wait::{WaitStatus, waitpid};
use nix::unistd::{self, ForkResult};

use crate::cmd::Cmd;
use crate::container_setup;
use crate::daemon::protocol::ExecRequest;
use crate::procfs::ProcStatus;
use crate::result::Result;

/// Execute a command in the container namespace
///
/// This function performs the actual exec requested by a client.
/// It's called by the daemon after receiving and validating an ExecRequest.
///
/// Architecture:
/// - Forks a child process to handle the exec
/// - Child enters all container namespaces and applies security context
/// - Child executes command via chroot to /var/lib/cntr
/// - Parent waits for child to complete and returns exit status
///
/// This allows the daemon to continue handling other exec requests.
pub(crate) fn execute_in_container(
    request: &ExecRequest,
    process_status: &ProcStatus,
) -> Result<()> {
    // Fork to handle the exec without blocking the daemon
    let fork_result = unsafe { unistd::fork().context("failed to fork for exec handler")? };

    match fork_result {
        ForkResult::Parent { child } => {
            // Parent: Wait for child to complete
            match waitpid(child, None) {
                Ok(WaitStatus::Exited(_, status)) => {
                    if status != 0 {
                        warn!("exec handler child exited with status {}", status);
                    }
                    Ok(())
                }
                Ok(status) => {
                    warn!("exec handler child terminated unexpectedly: {:?}", status);
                    Ok(())
                }
                Err(e) => {
                    warn!("failed to wait for exec handler child: {}", e);
                    Ok(())
                }
            }
        }
        ForkResult::Child => {
            // Child: Enter container namespaces and exec command
            if let Err(e) = exec_in_child(request, process_status) {
                warn!("failed to exec in container: {}", e);
                std::process::exit(1);
            }
            // exec_in_child never returns on success
            unreachable!()
        }
    }
}

/// Child process logic: Enter container namespaces and exec command
///
/// This function runs in the forked child process and:
/// 1. Uses shared container_setup to enter container and apply security context
/// 2. Creates Cmd with container environment
/// 3. Executes command via chroot
///
/// This function does NOT return on success (exec replaces process).
/// The PTY slave is already attached (inherited from attach child process).
fn exec_in_child(request: &ExecRequest, process_status: &ProcStatus) -> Result<()> {
    let container_pid = process_status.global_pid;

    // Enter container: cgroup, namespaces, security context (LSM, UID/GID, capabilities)
    container_setup::enter_container(container_pid, process_status)?;

    // Create command with container's environment
    let cmd = Cmd::new(
        request.command.clone(),
        request.arguments.clone(),
        container_pid,
        None,
    )
    .context("failed to create command for exec request")?;

    // Execute via chroot
    // This will NOT return - it replaces the current process
    cmd.exec_chroot()
        .context("failed to execute command in container")?;

    Ok(())
}
