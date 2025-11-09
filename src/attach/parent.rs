use anyhow::{Context, bail};
use nix::cmsg_space;
use nix::unistd::Pid;
use std::os::fd::RawFd;

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
) -> Result<std::convert::Infallible> {
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

    // Step 3: Forward PTY I/O and wait for child to exit
    // This will block until child exits, then propagate the exit status
    pty::forward_pty_and_wait(&pty_fd, child_pid)
}
