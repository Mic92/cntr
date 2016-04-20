use libc;
use types::{Error, Result};
use nix::{self, unistd};
use nix::errno::Errno;
use nix::sys::termios::*;
use nix::sys::select;
use std::{mem, ptr};
use std::fs::File;
use std::os::unix::prelude::*;
use std::io::{Read, Write};

#[link(name = "c")]
extern "C" {
    pub fn posix_openpt(flags: libc::c_int) -> libc::c_int;
    pub fn grantpt(fd: libc::c_int) -> libc::c_int;
    pub fn unlockpt(fd: libc::c_int) -> libc::c_int;
    pub fn ptsname(fd: libc::c_int) -> *mut libc::c_schar;
}

pub enum PtyFork {
    Parent {
        pid: libc::pid_t,
        pty_master: File,
        stdin_attr: Option<Termios>,
    },
    Child,
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
        return FilePair {
            from: from,
            to: to,
            buf: [8; libc::BUFSIZ as usize],
            write_offset: 0,
            read_offset: 0,
            state: FilePairState::Read,
        };
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
        match self.to.write(&self.buf[self.write_offset..self.read_offset]) {
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

        match select::select(highest + 1,
                             Some(&mut read_set),
                             Some(&mut write_set),
                             None,
                             None) {
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
                    if read_set.contains(pair.from.as_raw_fd()) {
                        if !pair.read() {
                            return;
                        };
                    }
                }
                FilePairState::Write => {
                    if write_set.contains(pair.to.as_raw_fd()) {
                        if !pair.write() {
                            return;
                        }
                    }
                }
            }
        }
    }
}

pub fn forward(pty: &File) {
    let stdin: File = unsafe { File::from_raw_fd(libc::STDIN_FILENO) };
    let stdout: File = unsafe { File::from_raw_fd(libc::STDOUT_FILENO) };
    shovel(&mut [FilePair::new(&stdin, pty), FilePair::new(pty, &stdout)]);
    mem::forget(stdin);
    mem::forget(stdout);
}

impl Drop for PtyFork {
    fn drop(&mut self) {
        match self {
            &mut PtyFork::Parent { ref stdin_attr, .. } => {
                if stdin_attr.is_some() {
                    match tcsetattr(libc::STDIN_FILENO, SetArg::TCSANOW, &stdin_attr.unwrap()) {
                        Err(err) => warn!("failed to restore stdin tty: {}", err),
                        _ => {}
                    };
                }
            }
            _ => {}
        }
    }
}

fn set_tty_raw(fd: RawFd) -> Result<Termios> {
    let orig_attr = tryfmt!(tcgetattr(fd), "failed to get termios attributes");

    let mut attr = orig_attr;
    attr.c_iflag.remove(IGNBRK | BRKINT | PARMRK | ISTRIP | INLCR | IGNCR | ICRNL | IXON);
    attr.c_oflag.remove(OPOST);
    attr.c_lflag.remove(ECHO | ECHONL | ICANON | ISIG | IEXTEN);
    attr.c_cflag.remove(CSIZE | PARENB);
    attr.c_cflag.insert(CS8);
    attr.c_cc[VMIN] = 1; // One character-at-a-time input
    attr.c_cc[VTIME] = 0; // with blocking read

    tryfmt!(tcsetattr(fd, SetArg::TCSAFLUSH, &attr),
            "failed to set termios attributes");
    return Ok(orig_attr);
}

pub fn fork() -> Result<PtyFork> {
    let pty_master = tryfmt!(open_ptm(), "open pty master");

    match tryfmt!(unistd::fork(), "fork()") {
        unistd::ForkResult::Parent { child } => setup_parent(child, pty_master),
        unistd::ForkResult::Child => {
            tryfmt!(attach_pts(pty_master), "attach to pty");
            Ok(PtyFork::Child)
        }
    }
}
#[repr(C)]
struct winsize {
    ws_row: libc::c_ushort,
    ws_col: libc::c_ushort,
    ws_xpixel: libc::c_ushort,
    ws_ypixel: libc::c_ushort,
}

fn get_winsize(term_fd: RawFd) -> winsize {
    use std::mem::zeroed;
    unsafe {
        let mut ws: winsize = zeroed();
        match libc::ioctl(term_fd, libc::TIOCGWINSZ, &mut ws) {
            0 => ws,
            _ => {
                winsize {
                    ws_row: 80,
                    ws_col: 25,
                    ws_xpixel: 0,
                    ws_ypixel: 0,
                }
            }
        }
    }
}


fn resize_pty(pty_master: &File) {
    unsafe {
        libc::ioctl(pty_master.as_raw_fd(),
                    libc::TIOCSWINSZ,
                    &mut get_winsize(libc::STDOUT_FILENO));
    }
}

fn setup_parent(pid: libc::pid_t, pty_master: libc::c_int) -> Result<PtyFork> {
    let mut parent = PtyFork::Parent {
        pid: pid,
        pty_master: unsafe { File::from_raw_fd(pty_master) },
        stdin_attr: None,
    };

    if unsafe { libc::isatty(libc::STDIN_FILENO as i32) } == 0 {
        return Ok(parent);
    }

    if let PtyFork::Parent { ref mut stdin_attr, ref mut pty_master, .. } = parent {
        resize_pty(pty_master);
        *stdin_attr = Some(tryfmt!(set_tty_raw(libc::STDIN_FILENO),
                                   "failed to set stdin tty into raw mode"));
    }

    return Ok(parent);
}

fn open_ptm() -> Result<RawFd> {
    let pty_master = unsafe_try!(posix_openpt(libc::O_RDWR), "posix_openpt()");

    unsafe_try!(grantpt(pty_master), "grantpt()");
    unsafe_try!(unlockpt(pty_master), "unlockpt()");

    Ok(pty_master as RawFd)
}

fn attach_pts(pty_master: libc::c_int) -> Result<()> {
    let pts_name = unsafe { ptsname(pty_master) };

    if (pts_name as *const i32) == ptr::null() {
        return errfmt!(nix::Error::Sys(Errno::last()), "ptsname()");
    }

    tryfmt!(unistd::close(pty_master), "cannot close master pty");
    unsafe_try!(libc::setsid(), "setsid()");

    let pty_slave = unsafe_try!(libc::open(pts_name, libc::O_RDWR, 0),
                                "cannot open slave pty");

    tryfmt!(unistd::dup2(pty_slave, libc::STDIN_FILENO),
            "cannot set pty as stdin");
    tryfmt!(unistd::dup2(pty_slave, libc::STDOUT_FILENO),
            "cannot set pty as stdout");
    tryfmt!(unistd::dup2(pty_slave, libc::STDERR_FILENO),
            "cannot set pty as stderr");

    tryfmt!(unistd::close(pty_slave), "cannot close slave pty");

    Ok(())
}
