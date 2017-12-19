use nix::{self, fcntl, errno};
use nix::sys::stat;
use std::fs::File;
use std::os::unix::prelude::*;
use tempdir::TempDir;
use types::{Error, Result};

pub fn open() -> Result<File> {
    let res = fcntl::open("/dev/fuse", fcntl::O_RDWR, stat::Mode::empty());

    match res {
        Ok(fd) => {
            let file = unsafe { File::from_raw_fd(fd) };
            return Ok(file);
        }
        Err(nix::Error::Sys(errno::Errno::ENOENT)) => {}
        Err(err) => return errfmt!(err, "failed to open /dev/fuse"),
    };

    // docker container lacks /dev/fuse
    let tempdir = tryfmt!(
        TempDir::new("cntr-fuse-fd"),
        "failed to create temporary directory for fuse node"
    );

    let fuse_path = tempdir.path().join("fuse");

    tryfmt!(
        stat::mknod(
            &fuse_path,
            stat::S_IFCHR,
            stat::S_IRUSR | stat::S_IWUSR,
            stat::makedev(10, 229),
        ),
        "failed to create temporary fuse character device"
    );

    let file = unsafe {
        File::from_raw_fd(tryfmt!(
            fcntl::open(&fuse_path, fcntl::O_RDWR, stat::Mode::empty()),
            "failed to open fuse device"
        ))
    };
    Ok(file)
}
