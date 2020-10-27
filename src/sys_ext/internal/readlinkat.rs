use nix::errno::Errno;
use nix::NixPath;
use std::ffi::{OsStr, OsString};
use std::os::unix::prelude::*;

fn readlinkat<'a, P: ?Sized + NixPath>(
    dirfd: RawFd,
    path: &P,
    buffer: &'a mut [u8],
) -> nix::Result<&'a OsStr> {
    let res = path.with_nix_path(|cstr| unsafe {
        libc::readlinkat(
            dirfd,
            cstr.as_ptr(),
            buffer.as_mut_ptr() as *mut libc::c_char,
            buffer.len() as libc::size_t,
        )
    })?;

    match Errno::result(res) {
        Err(err) => Err(err),
        Ok(len) => {
            if (len as usize) >= buffer.len() {
                Err(nix::Error::Sys(Errno::ENAMETOOLONG))
            } else {
                Ok(OsStr::from_bytes(&buffer[..(len as usize)]))
            }
        }
    }
}

pub fn fuse_readlinkat(fd: RawFd) -> nix::Result<OsString> {
    let mut buf = vec![0; (libc::PATH_MAX + 1) as usize];
    loop {
        match readlinkat(fd, "", &mut buf) {
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
