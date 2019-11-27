use libc::{self, winsize};
use nix::errno::Errno;
use nix::fcntl::OFlag;
use nix::pty::*;
use nix::sys::select;
use nix::sys::signal::{sigaction, SaFlags, SigAction, SigHandler, SigSet, SIGWINCH};
use nix::sys::stat;
use nix::sys::termios::SpecialCharacterIndices::*;
use nix::sys::termios::{
    tcgetattr, tcsetattr, ControlFlags, InputFlags, LocalFlags, OutputFlags, SetArg, Termios,
};
use nix::{self, fcntl, unistd};
use std::fs::File;
use std::io::{Read, Write};
use std::os::unix::prelude::*;
use types::{Error, Result};

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

struct RawTty {
    fd: RawFd,
    attr: Termios,
}

impl RawTty {
    fn new(stdin: RawFd) -> Result<RawTty> {
        let orig_attr = tryfmt!(tcgetattr(stdin), "failed to get termios attributes");

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

        tryfmt!(
            tcsetattr(stdin, SetArg::TCSAFLUSH, &attr),
            "failed to set termios attributes"
        );
        Ok(RawTty {
            fd: stdin,
            attr: orig_attr,
        })
    }
}

impl Drop for RawTty {
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
        let mut highest = 0;

        for pair in pairs.iter_mut() {
            let fd = match pair.state {
                FilePairState::Read => {
                    let raw_fd = pair.from.as_raw_fd();
                    read_set.insert(raw_fd);
                    raw_fd
                }
                FilePairState::Write => {
                    let raw_fd = pair.to.as_raw_fd();
                    write_set.insert(raw_fd);
                    raw_fd
                }
            };
            if highest < fd {
                highest = fd;
            }
        }

        match select::select(
            highest + 1,
            Some(&mut read_set),
            Some(&mut write_set),
            None,
            None,
        ) {
            Err(nix::Error::Sys(Errno::EINTR)) => {
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
                    if read_set.contains(pair.from.as_raw_fd()) && !pair.read() {
                        return;
                    }
                }
                FilePairState::Write => {
                    if write_set.contains(pair.to.as_raw_fd()) && !pair.write() {
                        return;
                    }
                }
            }
        }
    }
}

extern "C" fn handle_sigwinch(_: i32) {
    let fd = unsafe { PTY_MASTER_FD };
    if fd != -1 {
        resize_pty(fd);
    }
}

static mut PTY_MASTER_FD: i32 = -1;

pub fn forward(pty: &PtyMaster) -> Result<()> {
    let mut raw_tty = None;

    if unsafe { libc::isatty(libc::STDIN_FILENO as i32) } != 0 {
        resize_pty(pty.as_raw_fd());

        raw_tty = Some(tryfmt!(
            RawTty::new(libc::STDIN_FILENO),
            "failed to set stdin tty into raw mode"
        ))
    };

    unsafe { PTY_MASTER_FD = pty.as_raw_fd() };
    let sig_action = SigAction::new(
        SigHandler::Handler(handle_sigwinch),
        SaFlags::empty(),
        SigSet::empty(),
    );
    tryfmt!(
        unsafe { sigaction(SIGWINCH, &sig_action) },
        "failed to install SIGWINCH handler"
    );

    let stdin: File = unsafe { File::from_raw_fd(libc::STDIN_FILENO) };
    let stdout: File = unsafe { File::from_raw_fd(libc::STDOUT_FILENO) };
    let pty_file: File = unsafe { File::from_raw_fd(pty.as_raw_fd()) };
    shovel(&mut [
        FilePair::new(&stdin, &pty_file),
        FilePair::new(&pty_file, &stdout),
    ]);
    stdin.into_raw_fd();
    stdout.into_raw_fd();
    pty_file.into_raw_fd();

    unsafe { PTY_MASTER_FD = -1 };

    raw_tty.map(drop);

    Ok(())
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

pub fn open_ptm() -> Result<PtyMaster> {
    let pty_master = tryfmt!(posix_openpt(OFlag::O_RDWR), "posix_openpt()");

    tryfmt!(grantpt(&pty_master), "grantpt()");
    tryfmt!(unlockpt(&pty_master), "unlockpt()");

    Ok(pty_master)
}

pub fn attach_pts(pty_master: &PtyMaster) -> nix::Result<()> {
    let pts_name = ptsname_r(pty_master)?;

    unistd::setsid()?;

    let pty_slave = fcntl::open(pts_name.as_str(), OFlag::O_RDWR, stat::Mode::empty())?;

    unistd::dup2(pty_slave, libc::STDIN_FILENO)?;
    unistd::dup2(pty_slave, libc::STDOUT_FILENO)?;
    unistd::dup2(pty_slave, libc::STDERR_FILENO)?;

    unistd::close(pty_slave)?;

    Ok(())
}
