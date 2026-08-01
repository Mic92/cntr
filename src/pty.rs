use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use log::warn;
use rustix::event::{PollFd, PollFlags, Timespec, poll};
use rustix::fd::{AsFd, BorrowedFd, OwnedFd};
use rustix::fs::{Mode, OFlags, open};
use rustix::io::{Errno, dup};
use rustix::process::{
    Pid, Signal, WaitOptions, getpid, ioctl_tiocsctty, kill_process, setsid, waitpid,
};
use rustix::pty::{OpenptFlags, grantpt, openpt, ptsname, unlockpt};
use rustix::runtime::exit_group;
use rustix::stdio::{dup2_stderr, dup2_stdin, dup2_stdout, stdin, stdout};
use rustix::termios::{
    ControlModes, InputModes, LocalModes, OptionalActions, OutputModes, SpecialCodeIndex, Termios,
    Winsize, isatty, tcgetattr, tcgetwinsize, tcsetattr, tcsetwinsize,
};
use thiserror::Error;

#[derive(Debug, Error)]
pub(crate) enum PtyError {
    #[error("failed to get termios attributes")]
    GetTermios(#[source] Errno),
    #[error("failed to set termios attributes")]
    SetTermios(#[source] Errno),
    #[error("failed to duplicate {what}")]
    Dup {
        what: &'static str,
        #[source]
        source: Errno,
    },
    #[error("failed to open pty with posix_openpt()")]
    OpenPtyMaster(#[source] Errno),
    #[error("failed to grant pty access with grantpt()")]
    Grantpt(#[source] Errno),
    #[error("failed to unlock pty with unlockpt()")]
    Unlockpt(#[source] Errno),
    #[error("failed to get PTY slave name from master")]
    Ptsname(#[source] Errno),
    #[error("PTY slave name is not valid UTF-8")]
    PtsnameNotUtf8,
    #[error("failed to create new session for PTY")]
    Setsid(#[source] Errno),
    #[error("failed to open PTY slave at {path}")]
    OpenPtySlave {
        path: String,
        #[source]
        source: Errno,
    },
    #[error("failed to redirect {what} to PTY slave")]
    Dup2 {
        what: &'static str,
        #[source]
        source: Errno,
    },
    #[error("waitpid failed")]
    WaitPid(#[source] Errno),
    #[error("unexpected wait event: {0}")]
    UnexpectedWaitEvent(String),
}

const BUF_SIZE: usize = 8192;

enum FilePairState {
    Write,
    Read,
}

struct FilePair<'a> {
    from: &'a OwnedFd,
    to: &'a OwnedFd,
    buf: [u8; BUF_SIZE],
    read_offset: usize,
    write_offset: usize,
    state: FilePairState,
}

impl<'a> FilePair<'a> {
    fn new(from: &'a OwnedFd, to: &'a OwnedFd) -> FilePair<'a> {
        FilePair {
            from,
            to,
            buf: [0; BUF_SIZE],
            write_offset: 0,
            read_offset: 0,
            state: FilePairState::Read,
        }
    }
    fn read(&mut self) -> bool {
        match rustix::io::read(self.from, &mut self.buf) {
            Ok(read) => {
                self.read_offset = read;
                self.write()
            }
            Err(_) => false,
        }
    }
    fn write(&mut self) -> bool {
        match rustix::io::write(self.to, &self.buf[self.write_offset..self.read_offset]) {
            Ok(written) => {
                self.write_offset += written;
                if self.write_offset >= self.read_offset {
                    self.read_offset = 0;
                    self.write_offset = 0;
                    self.state = FilePairState::Read;
                } else {
                    self.state = FilePairState::Write;
                };
                true
            }
            Err(_) => false,
        }
    }
}

struct RawTty<'a> {
    fd: BorrowedFd<'a>,
    attr: Termios,
}

impl<'a> RawTty<'a> {
    fn new(stdin: BorrowedFd<'a>) -> Result<RawTty<'a>, PtyError> {
        let orig_attr = tcgetattr(stdin).map_err(PtyError::GetTermios)?;

        let mut attr = orig_attr.clone();
        attr.input_modes.remove(
            InputModes::IGNBRK
                | InputModes::BRKINT
                | InputModes::PARMRK
                | InputModes::ISTRIP
                | InputModes::INLCR
                | InputModes::IGNCR
                | InputModes::ICRNL
                | InputModes::IXON,
        );
        attr.output_modes.remove(OutputModes::OPOST);
        attr.local_modes.remove(
            LocalModes::ECHO
                | LocalModes::ECHONL
                | LocalModes::ICANON
                | LocalModes::ISIG
                | LocalModes::IEXTEN,
        );
        attr.control_modes
            .remove(ControlModes::CSIZE | ControlModes::PARENB);
        attr.control_modes.insert(ControlModes::CS8);
        attr.special_codes[SpecialCodeIndex::VMIN] = 1; // One character-at-a-time input
        attr.special_codes[SpecialCodeIndex::VTIME] = 0; // with blocking read

        tcsetattr(stdin, OptionalActions::Flush, &attr).map_err(PtyError::SetTermios)?;
        Ok(RawTty {
            fd: stdin,
            attr: orig_attr,
        })
    }
}

impl Drop for RawTty<'_> {
    fn drop(&mut self) {
        let _ = tcsetattr(self.fd, OptionalActions::Now, &self.attr);
    }
}

/// Forward data between the file pairs until one side is closed.
///
/// If `resize` is set, the terminal window size is propagated from stdout to
/// the given PTY master whenever it changes. We poll for size changes instead
/// of installing a SIGWINCH handler so that no signal handling (which would
/// require libc or a per-architecture signal trampoline) is needed.
fn shovel(pairs: &mut [FilePair], resize: Option<BorrowedFd>) {
    let mut last_winsize = resize.map(|pty| {
        let ws = get_winsize();
        set_winsize(pty, ws);
        ws
    });

    // Wake up regularly to check for terminal resizes.
    let poll_timeout = Timespec {
        tv_sec: 0,
        tv_nsec: 100_000_000,
    };
    let timeout = resize.map(|_| &poll_timeout);

    loop {
        let revents: Vec<PollFlags> = {
            let mut poll_fds: Vec<PollFd> = pairs
                .iter()
                .map(|pair| match pair.state {
                    FilePairState::Read => PollFd::new(pair.from, PollFlags::IN),
                    FilePairState::Write => PollFd::new(pair.to, PollFlags::OUT),
                })
                .collect();

            match poll(&mut poll_fds, timeout) {
                Err(Errno::INTR) => continue,
                Err(_) => return,
                Ok(_) => {}
            }

            poll_fds.iter().map(PollFd::revents).collect()
        };

        if let (Some(pty), Some(last)) = (resize, last_winsize.as_mut()) {
            let current = get_winsize();
            if current.ws_row != last.ws_row || current.ws_col != last.ws_col {
                set_winsize(pty, current);
                *last = current;
            }
        }

        for (pair, revents) in pairs.iter_mut().zip(revents) {
            if revents.is_empty() {
                continue;
            }
            let more = match pair.state {
                FilePairState::Read => pair.read(),
                FilePairState::Write => pair.write(),
            };
            if !more {
                return;
            }
        }
    }
}

pub(crate) fn forward<T: AsFd>(pty: &T) -> Result<(), PtyError> {
    let is_tty = isatty(unsafe { stdin() });
    let _raw_tty = if is_tty {
        Some(RawTty::new(unsafe { stdin() })?)
    } else {
        None
    };

    // Duplicate FDs so each end owns its own FD and can be safely closed
    // This prevents double-close bugs when the original FD owners are dropped
    let dup_fd = |what, fd: BorrowedFd| -> Result<OwnedFd, PtyError> {
        dup(fd).map_err(|source| PtyError::Dup { what, source })
    };
    let stdin_file = dup_fd("stdin", unsafe { stdin() })?;
    let stdout_file = dup_fd("stdout", unsafe { stdout() })?;
    let pty_file = dup_fd("pty master", pty.as_fd())?;

    shovel(
        &mut [
            FilePair::new(&stdin_file, &pty_file),
            FilePair::new(&pty_file, &stdout_file),
        ],
        is_tty.then(|| pty.as_fd()),
    );

    Ok(())
}

/// Forward PTY I/O and wait for child process to exit, propagating exit status.
///
/// This function:
/// 1. Forwards PTY I/O between stdin/stdout and the PTY (blocks until child exits)
/// 2. Waits for the child process to exit with job control support
/// 3. Propagates the child's exit status to the current process
///
/// Job control handling:
/// - If child is stopped (Ctrl+Z), stops parent too
/// - When parent resumes, resumes the child
///
/// This function never returns - it always exits the process.
pub(crate) fn forward_pty_and_wait<T: AsFd>(
    pty: &T,
    child_pid: Pid,
) -> Result<core::convert::Infallible, PtyError> {
    // Forward PTY I/O between stdin/stdout and the PTY
    // This will block until child exits or PTY closes
    let _ = forward(pty);

    // Wait for child to exit and propagate exit status
    // Loop to handle job control signals (SIGSTOP, SIGCONT) and EINTR
    loop {
        match waitpid(Some(child_pid), WaitOptions::UNTRACED) {
            Ok(Some((_, status))) => {
                if status.stopped() {
                    // Child was stopped (Ctrl+Z) - stop ourselves and resume child when we resume
                    let _ = kill_process(getpid(), Signal::STOP);
                    let _ = kill_process(child_pid, Signal::CONT);
                } else if let Some(sig) = status.terminating_signal() {
                    // Child was signaled - propagate signal and exit
                    if let Some(signal) = Signal::from_named_raw(sig) {
                        let _ = kill_process(getpid(), signal);
                    }
                    exit_group(128 + sig);
                } else if let Some(code) = status.exit_status() {
                    // Child exited normally - exit with same status
                    exit_group(code);
                } else {
                    return Err(PtyError::UnexpectedWaitEvent(format!("{:?}", status)));
                }
            }
            // Interrupted or nothing to report yet, continue waiting
            Ok(None) | Err(Errno::INTR) => continue,
            Err(e) => return Err(PtyError::WaitPid(e)),
        }
    }
}

fn get_winsize() -> Winsize {
    tcgetwinsize(unsafe { stdout() }).unwrap_or(Winsize {
        ws_row: 80,
        ws_col: 25,
        ws_xpixel: 0,
        ws_ypixel: 0,
    })
}

fn set_winsize(pty_master: BorrowedFd, ws: Winsize) {
    let _ = tcsetwinsize(pty_master, ws);
}

pub(crate) fn open_ptm() -> Result<OwnedFd, PtyError> {
    let pty_master = openpt(OpenptFlags::RDWR).map_err(PtyError::OpenPtyMaster)?;

    grantpt(&pty_master).map_err(PtyError::Grantpt)?;
    unlockpt(&pty_master).map_err(PtyError::Unlockpt)?;

    Ok(pty_master)
}

pub(crate) fn attach_pts(pty_master: &OwnedFd) -> Result<(), PtyError> {
    let pts_name = ptsname(pty_master, Vec::new())
        .map_err(PtyError::Ptsname)?
        .into_string()
        .map_err(|_| PtyError::PtsnameNotUtf8)?;

    setsid().map_err(PtyError::Setsid)?;

    let pty_slave = open(pts_name.as_str(), OFlags::RDWR, Mode::empty()).map_err(|source| {
        PtyError::OpenPtySlave {
            path: pts_name.clone(),
            source,
        }
    })?;

    // Set the PTY slave as the controlling terminal for this session
    // This is required for job control to work properly
    if let Err(err) = ioctl_tiocsctty(&pty_slave) {
        // If TIOCSCTTY fails, just warn but continue - job control may not work
        // but the command will still execute
        warn!("Failed to set controlling terminal: {}", err);
    }

    dup2_stdin(&pty_slave).map_err(|source| PtyError::Dup2 {
        what: "stdin",
        source,
    })?;
    dup2_stdout(&pty_slave).map_err(|source| PtyError::Dup2 {
        what: "stdout",
        source,
    })?;
    dup2_stderr(&pty_slave).map_err(|source| PtyError::Dup2 {
        what: "stderr",
        source,
    })?;

    Ok(())
}
