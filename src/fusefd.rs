use nix::{self, fcntl, errno};
use nix::fcntl::OFlag;
use nix::sys::stat::{self, SFlag, Mode};
use std::fs::File;
use std::os::unix::prelude::*;
use types::{Error, Result};

pub fn open() -> Result<File> {
    let res = fcntl::open("/dev/fuse", OFlag::O_RDWR, stat::Mode::empty());

    match res {
        Ok(fd) => {
            let file = unsafe { File::from_raw_fd(fd) };
            return Ok(file);
        }
        Err(nix::Error::Sys(errno::Errno::ENOENT)) => {}
        Err(err) => return errfmt!(err, "failed to open /dev/fuse"),
    };

    tryfmt!(
        stat::mknod(
            "/dev/fuse",
            SFlag::S_IFCHR,
            Mode::S_IRUSR | Mode::S_IWUSR,
            stat::makedev(10, 229),
        ),
        "failed to create temporary fuse character device"
    );

    let file = unsafe {
        File::from_raw_fd(tryfmt!(
            fcntl::open("/dev/fuse", OFlag::O_RDWR, stat::Mode::empty()),
            "failed to open fuse device"
        ))
    };
    Ok(file)
}
