use nix::fcntl::OFlag;
use nix::sys::stat::{self, Mode, SFlag};
use nix::{self, errno, fcntl};
use simple_error::{bail, try_with};
use std::fs::File;

use crate::result::Result;

pub fn open() -> Result<File> {
    let res = fcntl::open("/dev/fuse", OFlag::O_RDWR, stat::Mode::empty());

    match res {
        Ok(fd) => {
            let file = File::from(fd);
            return Ok(file);
        }
        Err(errno::Errno::ENOENT) => {}
        Err(err) => bail!("failed to open /dev/fuse: {}", err),
    };

    try_with!(
        stat::mknod(
            "/dev/fuse",
            SFlag::S_IFCHR,
            Mode::S_IRUSR | Mode::S_IWUSR,
            stat::makedev(10, 229),
        ),
        "failed to create temporary fuse character device"
    );

    let file = File::from(try_with!(
        fcntl::open("/dev/fuse", OFlag::O_RDWR, stat::Mode::empty()),
        "failed to open fuse device"
    ));
    Ok(file)
}
