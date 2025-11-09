use anyhow::{Context, bail};
use nix::sys::signal::{self, Signal};
use nix::sys::wait::{WaitPidFlag, WaitStatus, waitpid};
use nix::unistd::Pid;
use nix::{cmsg_space, unistd};
use std::os::fd::RawFd;
use std::process;

use crate::procfs::ProcStatus;

use crate::ipc;
use crate::pty;
use crate::result::Result;

/// Parent process logic for mount API attach
///
/// The parent stays in the host namespace and:
/// 1. Waits for child to assemble mount hierarchy and signal completion
/// 2. Receives PTY FD from child
/// 3. Forwards PTY I/O between child and terminal
/// 4. Manages child lifecycle (signals, exit status)
pub(crate) fn run(
    child_pid: Pid,
    _process_status: &ProcStatus,
    socket: &ipc::Socket,
) -> Result<()> {
    // Step 1: Wait for child to assemble mount hierarchy and signal completion
    // The child will send: ready signal + PTY fd
    let mut cmsgspace = cmsg_space!([RawFd; 1]);
    let (msg_buf, mut fds) = socket
        .receive::<std::fs::File>(1, &mut cmsgspace)
        .context("failed to receive ready signal from child")?;

    if msg_buf.is_empty() || msg_buf[0] != b'R' {
        bail!("child did not send ready signal");
    }

    // Step 2: Receive PTY fd from child
    if fds.is_empty() {
        bail!("expected PTY fd from child, got none");
    }
    let pty_fd = fds.remove(0);

    // Step 3: Forward PTY I/O
    // This will block until child exits or PTY closes
    let _ = pty::forward(&pty_fd);

    // Step 4: Wait for child to exit and propagate exit status
    loop {
        match waitpid(child_pid, Some(WaitPidFlag::WUNTRACED)) {
            Ok(WaitStatus::Stopped(child, _)) => {
                // Child was stopped (SIGSTOP, SIGTSTP, SIGTTIN, SIGTTOU, etc.)
                // Stop ourselves and resume child when we resume
                let _ = signal::kill(unistd::getpid(), Signal::SIGSTOP);
                let _ = signal::kill(child, Signal::SIGCONT);
            }
            Ok(WaitStatus::Signaled(_, sig, _)) => {
                // Child was terminated by a signal - propagate it to ourselves
                signal::kill(unistd::getpid(), sig)
                    .with_context(|| format!("failed to send signal {:?} to own process", sig))?;
            }
            Ok(WaitStatus::Exited(_, status)) => {
                // Child exited normally - exit with same status
                process::exit(status);
            }
            Ok(what) => {
                bail!("unexpected wait event: {:?}", what);
            }
            Err(nix::errno::Errno::EINTR) => {
                // Interrupted by signal, continue
                continue;
            }
            Err(e) => {
                return Err(e).context("waitpid failed");
            }
        }
    }
}
