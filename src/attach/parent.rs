use anyhow::{Context, bail};
use nix::poll::{PollFd, PollFlags, PollTimeout};
use nix::sys::signal::{self, Signal};
use nix::sys::wait::{WaitPidFlag, WaitStatus, waitpid};
use nix::unistd::Pid;
use nix::{cmsg_space, unistd};
use simple_error::{bail, try_with};
use std::os::fd::{AsFd, IntoRawFd, RawFd};
use std::process;

use crate::procfs::ProcStatus;

use crate::daemon;
use crate::ipc;
use crate::pty;
use crate::result::Result;

/// Parent process logic for mount API attach (T017, T030, T031)
///
/// The parent stays in the host namespace and:
/// 1. Waits for child to assemble mount hierarchy and signal completion
/// 2. Receives PTY FD and daemon socket FD from child
/// 3. Listens for and handles exec requests from cntr exec clients
/// 4. Forwards PTY I/O between child and terminal
/// 5. Manages child lifecycle (signals, exit status)
pub(crate) fn run(child_pid: Pid, process_status: &ProcStatus, socket: &ipc::Socket) -> Result<()> {
    // Step 1: Wait for child to assemble mount hierarchy and signal completion
    // The child will send: ready signal + PTY fd + daemon socket fd
    let mut cmsgspace = cmsg_space!([RawFd; 2]);
    let (msg_buf, mut fds) = try_with!(
        socket.receive::<std::fs::File>(1, &mut cmsgspace),
        "failed to receive ready signal from child"
    );

    if msg_buf.is_empty() || msg_buf[0] != b'R' {
        bail!("child did not send ready signal");
    }

    // Step 4: Receive PTY fd and daemon socket fd from child
    if fds.len() != 2 {
        bail!(
            "expected 2 fds (pty + daemon socket) from child, got {}",
            fds.len()
        );
    }
    let pty_fd = fds.remove(0);
    let daemon_fd = fds.remove(0);

    // Step 4.5: Wrap daemon socket FD received from child
    // The child created this socket at /var/lib/cntr/.exec.sock in the staging tmpfs
    // We receive the FD here and will use it to accept exec requests
    let daemon_sock = unsafe {
        daemon::DaemonSocket::from_raw_fd(daemon_fd.into_raw_fd(), process_status.clone())
    };

    // Step 5: Main event loop - forward PTY and handle exec requests (T031)
    // This handles:
    // - Terminal I/O forwarding
    // - Daemon socket connections (exec requests)
    // - Signal propagation (SIGSTOP, SIGCONT)
    // - Child exit status
    //
    // We use poll() to wait for activity on either the PTY or the daemon socket

    loop {
        // Set up poll file descriptors
        let mut poll_fds = [
            PollFd::new(pty_fd.as_fd(), PollFlags::POLLIN),
            PollFd::new(daemon_sock.as_fd(), PollFlags::POLLIN),
        ];

        // Poll with a timeout to periodically check child status
        // Timeout of 100ms allows responsive child monitoring
        let timeout = PollTimeout::try_from(100).unwrap();
        let poll_result = nix::poll::poll(&mut poll_fds, timeout);

        match poll_result {
            Ok(_) => {
                // Check if PTY has data
                if let Some(revents) = poll_fds[0].revents()
                    && revents.contains(PollFlags::POLLIN)
                {
                    // Forward PTY output
                    try_with!(pty::forward(&pty_fd), "failed to forward terminal output");
                }

                // Check if daemon socket has incoming connection
                if let Some(revents) = poll_fds[1].revents()
                    && revents.contains(PollFlags::POLLIN)
                {
                    // Try to accept and handle exec request
                    let _ = daemon_sock.try_accept();
                }
            }
            Err(nix::errno::Errno::EINTR) => {
                // Interrupted by signal, continue to check child status
            }
            Err(e) => {
                try_with!(Err(e), "poll failed");
            }
        }

        // Check child status (non-blocking)
        match waitpid(
            child_pid,
            Some(WaitPidFlag::WUNTRACED | WaitPidFlag::WNOHANG),
        ) {
            Ok(WaitStatus::StillAlive) => {
                // Child still running, continue loop
                continue;
            }
            Ok(WaitStatus::Signaled(child, Signal::SIGSTOP, _)) => {
                // Child was stopped - stop ourselves and resume child when we resume
                let _ = signal::kill(unistd::getpid(), Signal::SIGSTOP);
                let _ = signal::kill(child, Signal::SIGCONT);
            }
            Ok(WaitStatus::Signaled(_, signal, _)) => {
                // Child received a signal - propagate it to ourselves
                try_with!(
                    signal::kill(unistd::getpid(), signal),
                    "failed to send signal {:?} to our own process",
                    signal
                );
            }
            Ok(WaitStatus::Exited(_, status)) => {
                // Child exited normally - exit with same status
                process::exit(status);
            }
            Ok(what) => {
                // Unexpected wait event
                bail!("unexpected wait event: {:?}", what);
            }
            Err(e) => {
                // waitpid failed
                return try_with!(Err(e), "waitpid failed");
            }
        }
    }
}
