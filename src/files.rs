use nix::fcntl::OFlag;
use std::fs::File;
use std::os::unix::prelude::*;
use std::path::Path;

#[derive(PartialOrd, PartialEq)]
pub enum FdState {
    None,
    Readable,
    ReadWritable,
}

pub fn fd_path(fd: &Fd) -> String {
    format!("/proc/self/fd/{}", fd.raw())
}

pub fn mkdir_p<P: AsRef<Path>>(path: &P) -> io::Result<()> {
    if let Err(e) = create_dir_all(path) {
        if e.kind() != io::ErrorKind::AlreadyExists {
            return Err(e);
        }
    }
    Ok(())
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
            state,
        }
    }
    pub fn raw(&self) -> RawFd {
        self.file.as_raw_fd()
    }
    pub fn path(&self) -> String {
        fd_path(self)
    }
}
