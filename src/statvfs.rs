use std::os::unix::io::RawFd;
use std::mem;
use libc;
use nix::{Result, Errno};

pub fn fstatvfs(fd: RawFd) -> Result<libc::statvfs> {
    let mut s = unsafe { mem::zeroed() };
    let res = unsafe { libc::fstatvfs(fd, &mut s) };
    Errno::result(res).map(|_| s)
}
