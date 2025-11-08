use anyhow::{Context, bail};
use nix::poll::{PollFd, PollFlags, PollTimeout};
use nix::sys::signal::{self, Signal};
use nix::sys::wait::{WaitPidFlag, WaitStatus, waitpid};
use nix::unistd::Pid;
use nix::{cmsg_space, unistd};
use std::fs::File;
use std::os::fd::{AsFd, AsRawFd, FromRawFd, IntoRawFd, RawFd};
use std::process;
use std::thread;

use crate::procfs::ProcStatus;

use crate::daemon;
use crate::ipc;
use crate::pty;
use crate::result::Result;

/// Parent process logic for mount API attach
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
    let (msg_buf, mut fds) = socket
        .receive::<std::fs::File>(1, &mut cmsgspace)
        .context("failed to receive ready signal from child")?;

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
    // Also pass the PTY master FD so daemon-executed commands can attach to the PTY slave
    let daemon_sock = unsafe {
        daemon::DaemonSocket::from_raw_fd(
            daemon_fd.into_raw_fd(),
            process_status.clone(),
            pty_fd.as_raw_fd(),
        )
    };

    // Step 5: Forward PTY I/O in a thread and handle daemon socket connections

    // Duplicate the PTY FD for the forwarding thread
    let pty_fd_dup = unsafe { libc::dup(pty_fd.as_raw_fd()) };
    if pty_fd_dup < 0 {
        bail!("failed to duplicate PTY file descriptor");
    }

    // Spawn thread to handle PTY forwarding
    // This thread will exit naturally when the PTY closes (child exits)
    // or when the process exits
    let _pty_thread = thread::spawn(move || {
        let pty_file: File = unsafe { File::from_raw_fd(pty_fd_dup) };
        if let Err(e) = pty::forward(&pty_file) {
            eprintln!("PTY forwarding error: {}", e);
        }
    });

    // Main thread: handle daemon socket connections and monitor child
    // Use pidfd to efficiently wait for both daemon socket and child process
    let pidfd = unsafe {
        let fd = libc::syscall(libc::SYS_pidfd_open, child_pid.as_raw(), 0);
        if fd < 0 {
            bail!("failed to open pidfd for child process");
        }
        std::os::fd::OwnedFd::from_raw_fd(fd as RawFd)
    };

    loop {
        // Poll both daemon socket and child pidfd
        let mut poll_fds = [
            PollFd::new(daemon_sock.as_fd(), PollFlags::POLLIN),
            PollFd::new(pidfd.as_fd(), PollFlags::POLLIN),
        ];

        match nix::poll::poll(&mut poll_fds, PollTimeout::NONE) {
            Ok(_) => {
                // Check if daemon socket has activity
                if let Some(revents) = poll_fds[0].revents()
                    && revents.contains(PollFlags::POLLIN)
                {
                    let _ = daemon_sock.try_accept();
                }

                // Check if child has exited or signaled
                if let Some(revents) = poll_fds[1].revents()
                    && (revents.contains(PollFlags::POLLIN) || revents.contains(PollFlags::POLLHUP))
                {
                    // Child has changed state - check with waitpid
                    match waitpid(
                        child_pid,
                        Some(WaitPidFlag::WUNTRACED | WaitPidFlag::WNOHANG),
                    ) {
                        Ok(WaitStatus::StillAlive) => {
                            // False alarm or spurious wakeup, continue
                            continue;
                        }
                        Ok(WaitStatus::Signaled(child, Signal::SIGSTOP, _)) => {
                            // Child was stopped - stop ourselves and resume child when we resume
                            let _ = signal::kill(unistd::getpid(), Signal::SIGSTOP);
                            let _ = signal::kill(child, Signal::SIGCONT);
                        }
                        Ok(WaitStatus::Signaled(_, sig, _)) => {
                            // Child received a signal - propagate it to ourselves
                            signal::kill(unistd::getpid(), sig).with_context(|| {
                                format!("failed to send signal {:?} to own process", sig)
                            })?;
                        }
                        Ok(WaitStatus::Exited(_, status)) => {
                            // Child exited normally - exit immediately
                            // PTY thread will be cleaned up automatically when process exits
                            process::exit(status);
                        }
                        Ok(what) => {
                            bail!("unexpected wait event: {:?}", what);
                        }
                        Err(e) => {
                            return Err(e).context("waitpid failed");
                        }
                    }
                }
            }
            Err(nix::errno::Errno::EINTR) => {
                // Interrupted by signal, continue
                continue;
            }
            Err(e) => {
                return Err(e).context("poll failed");
            }
        }
    }
}
