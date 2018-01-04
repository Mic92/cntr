use nix::unistd;
use nix::fcntl::OFlag;
use std::os::unix::prelude::*;

#[derive(PartialOrd,PartialEq)]
pub enum FdState {
    None,
    Readable,
    ReadWritable
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
    pub number: RawFd,
    pub state: FdState,
}

impl Fd {
    pub fn raw(&self) -> RawFd {
        self.number
    }
}

impl Drop for Fd {
    fn drop(&mut self) {
        unistd::close(self.number).unwrap();
    }
}
