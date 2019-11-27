use libc;
use nix::errno::Errno;
use nix::Result;
use std::mem;
use std::os::unix::io::RawFd;

pub fn fstatvfs(fd: RawFd) -> Result<libc::statvfs64> {
    let mut s = unsafe { mem::zeroed() };
    let res = unsafe { libc::fstatvfs64(fd, &mut s) };
    Errno::result(res).map(|_| s)
}
