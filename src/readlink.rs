use libc;
use nix::{self, fcntl};
use nix::errno::Errno;
use std::ffi::OsString;
use std::os::unix::prelude::*;

pub fn readlinkat(fd: RawFd) -> nix::Result<OsString> {
    let mut buf = vec![0; (libc::PATH_MAX + 1) as usize];
    loop {
        match fcntl::readlinkat(fd, "", &mut buf) {
            Ok(target) => {
                return Ok(OsString::from(target));
            }
            Err(nix::Error::Sys(Errno::ENAMETOOLONG)) => {}
            Err(e) => return Err(e),
        };
        // Trigger the internal buffer resizing logic of `Vec` by requiring
        // more space than the current capacity. The length is guaranteed to be
        // the same as the capacity due to the if statement above.
        buf.reserve(1)
    }

}
