use rustix::process::Pid;

use crate::attach::AttachError;
use crate::ipc;
use crate::procfs::ProcStatus;
use crate::pty;

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
) -> Result<std::convert::Infallible, AttachError> {
    // Step 1: Wait for child to assemble mount hierarchy and signal completion
    // The child will send: ready signal + PTY fd
    let (msg_buf, mut fds) = socket.receive::<std::fs::File>(1)?;

    if msg_buf.is_empty() || msg_buf[0] != b'R' {
        return Err(AttachError::ChildNotReady);
    }

    // Step 2: Receive PTY fd from child
    if fds.is_empty() {
        return Err(AttachError::MissingPtyFd);
    }
    let pty_fd = fds.remove(0);

    // Step 3: Forward PTY I/O and wait for child to exit
    // This will block until child exits, then propagate the exit status
    Ok(pty::forward_pty_and_wait(&pty_fd, child_pid)?)
}
