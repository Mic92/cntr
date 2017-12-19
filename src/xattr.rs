use libc::{self, c_int};
use nix::{Result, NixPath};
use nix::errno::Errno;
use std::os::unix::io::RawFd;

pub fn fgetxattr<P: ?Sized + NixPath>(fd: RawFd, name: &P, buf: &mut [u8]) -> Result<usize> {
    let res = try!(unsafe {
        name.with_nix_path(|cstr| {
            libc::fgetxattr(
                fd,
                cstr.as_ptr(),
                buf.as_mut_ptr() as *mut libc::c_void,
                buf.len(),
            )
        })
    });
    Errno::result(res).map(|size| size as usize)
}

pub fn flistxattr(fd: RawFd, list: &mut [u8]) -> Result<usize> {
    let res = unsafe { libc::flistxattr(fd, list.as_mut_ptr() as *mut i8, list.len()) };
    Errno::result(res).map(|size| size as usize)
}

pub fn fsetxattr<P: ?Sized + NixPath>(fd: RawFd, name: &P, buf: &[u8], flags: c_int) -> Result<()> {
    let res = try!(unsafe {
        name.with_nix_path(|cstr| {
            libc::fsetxattr(
                fd,
                cstr.as_ptr(),
                buf.as_ptr() as *const libc::c_void,
                buf.len(),
                flags,
            )
        })
    });
    Errno::result(res).map(drop)
}

pub fn fremovexattr<P: ?Sized + NixPath>(fd: RawFd, name: &P) -> Result<()> {
    let res = try!(unsafe {
        name.with_nix_path(|cstr| libc::fremovexattr(fd, cstr.as_ptr()))
    });
    Errno::result(res).map(drop)
}
