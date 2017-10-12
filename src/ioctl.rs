
use libc::{self, c_ulong};
use std::os::unix::io::RawFd;
use nix::{Result, Errno};

pub fn ioctl_read(fd: RawFd, cmd: c_ulong, data: &mut [u8]) -> Result<()> {
    let res = unsafe { libc::ioctl(fd, cmd, data) };
    Errno::result(res).map(drop)
}

pub fn ioctl_write(fd: RawFd, cmd: c_ulong, data: &[u8]) -> Result<()> {
    let res = unsafe { libc::ioctl(fd, cmd, data) };
    Errno::result(res).map(drop)
}

pub fn ioctl(fd: RawFd, cmd: c_ulong) -> Result<()> {
    let res = unsafe { libc::ioctl(fd, cmd) };
    Errno::result(res).map(drop)
}
