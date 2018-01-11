use nix::fcntl::OFlag;
use std::fs::File;
use std::os::unix::prelude::*;

#[derive(PartialOrd, PartialEq)]
pub enum FdState {
    None,
    Readable,
    ReadWritable,
}

pub fn fd_path(fd: &Fd) -> String {
    format!("/proc/self/fd/{}", fd.raw())
}

impl From<OFlag> for FdState {
    fn from(flags: OFlag) -> FdState {
        if flags & OFlag::O_RDWR == OFlag::O_RDWR {
            FdState::ReadWritable
        } else if flags & OFlag::O_RDONLY == OFlag::O_RDONLY {
            FdState::Readable
        } else {
            FdState::None
        }
    }
}

pub struct Fd {
    pub file: File,
    pub state: FdState,
}

impl Fd {
    pub fn new(fd: RawFd, state: FdState) -> Fd {
        Fd {
            file: unsafe { File::from_raw_fd(fd) },
            state: state,
        }
    }
    pub fn raw(&self) -> RawFd {
        self.file.as_raw_fd()
    }
    pub fn path(&self) -> String {
        fd_path(self)
    }
}
