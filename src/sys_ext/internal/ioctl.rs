use libc::{self, c_ulong};
use nix::errno::Errno;
use std::os::unix::io::RawFd;

#[cfg(not(any(target_env = "musl")))]
pub fn ioctl_read(fd: RawFd, cmd: c_ulong, data: &mut [u8]) -> nix::Result<()> {
    let res = unsafe { libc::ioctl(fd, cmd, data) };
    Errno::result(res).map(drop)
}

#[cfg(target_env = "musl")]
pub fn ioctl_read(fd: RawFd, cmd: c_ulong, data: &mut [u8]) -> nix::Result<()> {
    let res = unsafe { libc::ioctl(fd, cmd as i32, data) };
    Errno::result(res).map(drop)
}

#[cfg(not(target_env = "musl"))]
pub fn ioctl_write(fd: RawFd, cmd: c_ulong, data: &[u8]) -> nix::Result<()> {
    let res = unsafe { libc::ioctl(fd, cmd, data) };
    Errno::result(res).map(drop)
}

#[cfg(target_env = "musl")]
pub fn ioctl_write(fd: RawFd, cmd: c_ulong, data: &[u8]) -> nix::Result<()> {
    let res = unsafe { libc::ioctl(fd, cmd as i32, data) };
    Errno::result(res).map(drop)
}

#[cfg(not(target_env = "musl"))]
pub fn ioctl(fd: RawFd, cmd: c_ulong) -> nix::Result<()> {
    let res = unsafe { libc::ioctl(fd, cmd) };
    Errno::result(res).map(drop)
}

#[cfg(target_env = "musl")]
pub fn ioctl(fd: RawFd, cmd: c_ulong) -> nix::Result<()> {
    let res = unsafe { libc::ioctl(fd, cmd as i32) };
    Errno::result(res).map(drop)
}
