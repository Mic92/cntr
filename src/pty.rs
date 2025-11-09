use anyhow::{Context, bail};
use libc::{self, winsize};
use log::warn;
use nix::errno::Errno;
use nix::fcntl::OFlag;
use nix::pty::*;
use nix::sys::select;
use nix::sys::signal::{SIGWINCH, SaFlags, SigAction, SigHandler, SigSet, sigaction};
use nix::sys::stat;
use nix::sys::termios::SpecialCharacterIndices::*;
use nix::sys::termios::{
    ControlFlags, InputFlags, LocalFlags, OutputFlags, SetArg, Termios, tcgetattr, tcsetattr,
};
use nix::{self, fcntl, unistd};
use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::{AsFd, AsRawFd, BorrowedFd};
use std::os::unix::prelude::*;
use std::sync::atomic::{AtomicI32, Ordering};

use crate::result::Result;

// Safe wrapper for the TIOCSCTTY ioctl
fn tiocsctty(fd: RawFd, arg: libc::c_int) -> nix::Result<libc::c_int> {
    let res = unsafe { libc::ioctl(fd, libc::TIOCSCTTY, arg) };
    Errno::result(res)
}

enum FilePairState {
    Write,
    Read,
}

struct FilePair<'a> {
    from: &'a File,
    to: &'a File,
    buf: [u8; libc::BUFSIZ as usize],
    read_offset: usize,
    write_offset: usize,
    state: FilePairState,
}

impl<'a> FilePair<'a> {
    fn new(from: &'a File, to: &'a File) -> FilePair<'a> {
        FilePair {
            from,
            to,
            buf: [8; libc::BUFSIZ as usize],
            write_offset: 0,
            read_offset: 0,
            state: FilePairState::Read,
        }
    }
    fn read(&mut self) -> bool {
        match self.from.read(&mut self.buf) {
            Ok(read) => {
                self.read_offset = read;
                self.write()
            }
            Err(_) => false,
        }
    }
    fn write(&mut self) -> bool {
        match self
            .to
            .write(&self.buf[self.write_offset..self.read_offset])
        {
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
    fn new(stdin: BorrowedFd<'a>) -> Result<RawTty<'a>> {
        let orig_attr = tcgetattr(stdin).context("failed to get termios attributes")?;

        let mut attr = orig_attr.clone();
        attr.input_flags.remove(
            InputFlags::IGNBRK
                | InputFlags::BRKINT
                | InputFlags::PARMRK
                | InputFlags::ISTRIP
                | InputFlags::INLCR
                | InputFlags::IGNCR
                | InputFlags::ICRNL
                | InputFlags::IXON,
        );
        attr.output_flags.remove(OutputFlags::OPOST);
        attr.local_flags.remove(
            LocalFlags::ECHO
                | LocalFlags::ECHONL
                | LocalFlags::ICANON
                | LocalFlags::ISIG
                | LocalFlags::IEXTEN,
        );
        attr.control_flags
            .remove(ControlFlags::CSIZE | ControlFlags::PARENB);
        attr.control_flags.insert(ControlFlags::CS8);
        attr.control_chars[VMIN as usize] = 1; // One character-at-a-time input
        attr.control_chars[VTIME as usize] = 0; // with blocking read

        tcsetattr(stdin, SetArg::TCSAFLUSH, &attr).context("failed to set termios attributes")?;
        Ok(RawTty {
            fd: stdin,
            attr: orig_attr,
        })
    }
}

impl Drop for RawTty<'_> {
    fn drop(&mut self) {
        let _ = tcsetattr(self.fd, SetArg::TCSANOW, &self.attr);
    }
}

fn shovel(pairs: &mut [FilePair]) {
    let mut read_set = select::FdSet::new();
    let mut write_set = select::FdSet::new();

    loop {
        read_set.clear();
        write_set.clear();
        let mut highest: Option<BorrowedFd> = None;

        for pair in pairs.iter_mut() {
            let fd = match pair.state {
                FilePairState::Read => {
                    let raw_fd = pair.from.as_fd();
                    read_set.insert(raw_fd);
                    raw_fd
                }
                FilePairState::Write => {
                    let raw_fd = pair.to.as_fd();
                    write_set.insert(raw_fd);
                    raw_fd
                }
            };
            match highest {
                Some(highest_fd) => {
                    if highest_fd.as_raw_fd() < fd.as_raw_fd() {
                        highest = Some(fd);
                    }
                }
                None => {
                    highest = Some(fd);
                }
            }
        }

        let highest = match highest {
            Some(fd) => fd,
            None => return,
        };

        match select::select(
            highest.as_raw_fd() + 1,
            Some(&mut read_set),
            Some(&mut write_set),
            None,
            None,
        ) {
            Err(Errno::EINTR) => {
                continue;
            }
            Err(_) => {
                return;
            }
            _ => {}
        }

        for pair in pairs.iter_mut() {
            match pair.state {
                FilePairState::Read => {
                    if read_set.contains(pair.from.as_fd()) && !pair.read() {
                        return;
                    }
                }
                FilePairState::Write => {
                    if write_set.contains(pair.to.as_fd()) && !pair.write() {
                        return;
                    }
                }
            }
        }
    }
}

extern "C" fn handle_sigwinch(_: i32) {
    let fd = PTY_MASTER_FD.load(Ordering::Relaxed);
    if fd != -1 {
        resize_pty(fd);
    }
}

static PTY_MASTER_FD: AtomicI32 = AtomicI32::new(-1);

pub(crate) fn forward<T: AsRawFd + AsFd>(pty: &T) -> Result<()> {
    let mut raw_tty = None;

    if unsafe { libc::isatty(libc::STDIN_FILENO) } != 0 {
        resize_pty(pty.as_raw_fd());

        raw_tty = Some(
            RawTty::new(unsafe { BorrowedFd::borrow_raw(libc::STDIN_FILENO) })
                .context("failed to set stdin tty into raw mode")?,
        )
    };

    PTY_MASTER_FD.store(pty.as_raw_fd(), Ordering::Relaxed);
    let sig_action = SigAction::new(
        SigHandler::Handler(handle_sigwinch),
        SaFlags::empty(),
        SigSet::empty(),
    );
    unsafe { sigaction(SIGWINCH, &sig_action) }.context("failed to install SIGWINCH handler")?;

    // Duplicate FDs so each File owns its own FD and can be safely closed
    // This prevents double-close bugs when the original FD owners are dropped
    let stdin_dup = unistd::dup(unsafe { BorrowedFd::borrow_raw(libc::STDIN_FILENO) })
        .context("failed to duplicate stdin")?;
    let stdout_dup = unistd::dup(unsafe { BorrowedFd::borrow_raw(libc::STDOUT_FILENO) })
        .context("failed to duplicate stdout")?;
    let pty_dup = unistd::dup(pty).context("failed to duplicate pty master")?;

    let stdin: File = unsafe { File::from_raw_fd(stdin_dup.into_raw_fd()) };
    let stdout: File = unsafe { File::from_raw_fd(stdout_dup.into_raw_fd()) };
    let pty_file: File = unsafe { File::from_raw_fd(pty_dup.into_raw_fd()) };

    shovel(&mut [
        FilePair::new(&stdin, &pty_file),
        FilePair::new(&pty_file, &stdout),
    ]);

    PTY_MASTER_FD.store(-1, Ordering::Relaxed);

    if let Some(_raw_tty) = raw_tty {
        drop(_raw_tty)
    }

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
pub(crate) fn forward_pty_and_wait<T: AsRawFd + AsFd>(
    pty: &T,
    child_pid: nix::unistd::Pid,
) -> Result<std::convert::Infallible> {
    use nix::sys::signal::{self, Signal};
    use nix::sys::wait::{WaitPidFlag, WaitStatus, waitpid};
    use nix::unistd;
    use std::process;

    // Forward PTY I/O between stdin/stdout and the PTY
    // This will block until child exits or PTY closes
    let _ = forward(pty);

    // Wait for child to exit and propagate exit status
    // Loop to handle job control signals (SIGSTOP, SIGCONT) and EINTR
    loop {
        match waitpid(child_pid, Some(WaitPidFlag::WUNTRACED)) {
            Ok(WaitStatus::Stopped(child, _)) => {
                // Child was stopped (Ctrl+Z) - stop ourselves and resume child when we resume
                let _ = signal::kill(unistd::getpid(), Signal::SIGSTOP);
                let _ = signal::kill(child, Signal::SIGCONT);
            }
            Ok(WaitStatus::Signaled(_, sig, _)) => {
                // Child was signaled - propagate signal and exit
                let _ = signal::kill(unistd::getpid(), sig);
                process::exit(128 + sig as i32);
            }
            Ok(WaitStatus::Exited(_, status)) => {
                // Child exited normally - exit with same status
                process::exit(status);
            }
            Ok(status) => {
                bail!("unexpected wait event: {:?}", status);
            }
            Err(nix::errno::Errno::EINTR) => {
                // Interrupted by signal, continue waiting
                continue;
            }
            Err(e) => {
                return Err(e).context("waitpid failed");
            }
        }
    }
}

fn get_winsize(term_fd: RawFd) -> winsize {
    use std::mem::zeroed;
    unsafe {
        let mut ws: winsize = zeroed();
        match libc::ioctl(term_fd, libc::TIOCGWINSZ, &mut ws) {
            0 => ws,
            _ => winsize {
                ws_row: 80,
                ws_col: 25,
                ws_xpixel: 0,
                ws_ypixel: 0,
            },
        }
    }
}

fn resize_pty(pty_master: RawFd) {
    unsafe {
        libc::ioctl(
            pty_master,
            libc::TIOCSWINSZ,
            &mut get_winsize(libc::STDOUT_FILENO),
        );
    }
}

pub(crate) fn open_ptm() -> Result<PtyMaster> {
    let pty_master =
        posix_openpt(OFlag::O_RDWR).context("failed to open pty with posix_openpt()")?;

    grantpt(&pty_master).context("failed to grant pty access with grantpt()")?;
    unlockpt(&pty_master).context("failed to unlock pty with unlockpt()")?;

    Ok(pty_master)
}

pub(crate) fn attach_pts(pty_master: &PtyMaster) -> Result<()> {
    let pts_name = ptsname_r(pty_master).context("failed to get PTY slave name from master")?;

    unistd::setsid().context("failed to create new session for PTY")?;

    let pty_slave = fcntl::open(pts_name.as_str(), OFlag::O_RDWR, stat::Mode::empty())
        .with_context(|| format!("failed to open PTY slave at {}", pts_name.as_str()))?;

    // Set the PTY slave as the controlling terminal for this session
    // This is required for job control to work properly
    if let Err(err) = tiocsctty(pty_slave.as_raw_fd(), 0) {
        // If TIOCSCTTY fails, just warn but continue - job control may not work
        // but the command will still execute
        warn!("Failed to set controlling terminal: {}", err);
    }

    unistd::dup2_stdin(&pty_slave).context("failed to redirect stdin to PTY slave")?;
    unistd::dup2_stdout(&pty_slave).context("failed to redirect stdout to PTY slave")?;
    unistd::dup2_stderr(&pty_slave).context("failed to redirect stderr to PTY slave")?;

    unistd::close(pty_slave).context("failed to close PTY slave after duplication")?;

    Ok(())
}
