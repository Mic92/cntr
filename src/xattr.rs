use files::Fd;
use fuse::FileType;
use libc::{self, c_int};
use nix::{Result, NixPath};
use nix::errno::Errno;
use readlink::readlinkat;
use std::ffi::OsStr;

pub fn setxattr(fd: &Fd, kind: FileType, name: &OsStr, value: &[u8], flags: u32) -> Result<()> {
    if kind == FileType::Symlink {
        let path = try!(readlinkat(fd.raw()));
        lsetxattr(path.as_os_str(), name, value, flags as i32)
    } else {
        setxattr_(fd.path().as_str(), name, value, flags as i32)
    }
}

pub fn removexattr(fd: &Fd, kind: FileType, name: &OsStr) -> Result<()> {
    if kind == FileType::Symlink {
        let path = try!(readlinkat(fd.raw()));
        lremovexattr(path.as_os_str(), name)
    } else {
        removexattr_(fd.path().as_str(), name)
    }
}

pub fn listxattr(fd: &Fd, kind: FileType, name: &mut [u8]) -> Result<usize> {
    if kind == FileType::Symlink {
        let path = try!(readlinkat(fd.raw()));
        llistxattr(path.as_os_str(), name)
    } else {
        listxattr_(fd.path().as_str(), name)
    }
}

pub fn getxattr(fd: &Fd, kind: FileType, name: &OsStr, buf: &mut [u8]) -> Result<usize> {
    if kind == FileType::Symlink {
        let path = try!(readlinkat(fd.raw()));
        lgetxattr(path.as_os_str(), name, buf)
    } else {
        getxattr_(fd.path().as_str(), name, buf)
    }
}

fn getxattr_<P1: ?Sized + NixPath, P2: ?Sized + NixPath>(
    path: &P1,
    name: &P2,
    buf: &mut [u8],
) -> Result<usize> {
    let res = try!(try!(unsafe {
        path.with_nix_path(|p| {
            name.with_nix_path(|n| {
                libc::getxattr(
                    p.as_ptr(),
                    n.as_ptr(),
                    buf.as_mut_ptr() as *mut libc::c_void,
                    buf.len(),
                )
            })
        })
    }));
    Errno::result(res).map(|size| size as usize)
}

fn lgetxattr<P1: ?Sized + NixPath, P2: ?Sized + NixPath>(
    path: &P1,
    name: &P2,
    buf: &mut [u8],
) -> Result<usize> {
    let res = try!(try!(unsafe {
        path.with_nix_path(|p| {
            name.with_nix_path(|n| {
                libc::lgetxattr(
                    p.as_ptr(),
                    n.as_ptr(),
                    buf.as_mut_ptr() as *mut libc::c_void,
                    buf.len(),
                )
            })
        })
    }));
    Errno::result(res).map(|size| size as usize)
}

fn listxattr_<P: ?Sized + NixPath>(path: &P, list: &mut [u8]) -> Result<usize> {
    let res = try!(unsafe {
        path.with_nix_path(|cstr| {
            libc::listxattr(cstr.as_ptr(), list.as_mut_ptr() as *mut i8, list.len())
        })
    });
    Errno::result(res).map(|size| size as usize)
}

fn llistxattr<P: ?Sized + NixPath>(path: &P, list: &mut [u8]) -> Result<usize> {
    let res = try!(unsafe {
        path.with_nix_path(|cstr| {
            libc::llistxattr(cstr.as_ptr(), list.as_mut_ptr() as *mut i8, list.len())
        })
    });
    Errno::result(res).map(|size| size as usize)
}

fn lsetxattr<P1: ?Sized + NixPath, P2: ?Sized + NixPath>(
    path: &P1,
    name: &P2,
    buf: &[u8],
    flags: c_int,
) -> Result<()> {
    let res = try!(try!(unsafe {
        path.with_nix_path(|p| {
            name.with_nix_path(|n| {
                libc::lsetxattr(
                    p.as_ptr(),
                    n.as_ptr(),
                    buf.as_ptr() as *const libc::c_void,
                    buf.len(),
                    flags,
                )
            })
        })
    }));
    Errno::result(res).map(drop)
}

fn setxattr_<P1: ?Sized + NixPath, P2: ?Sized + NixPath>(
    path: &P1,
    name: &P2,
    buf: &[u8],
    flags: c_int,
) -> Result<()> {
    let res = try!(try!(unsafe {
        path.with_nix_path(|p| {
            name.with_nix_path(|n| {
                libc::setxattr(
                    p.as_ptr(),
                    n.as_ptr(),
                    buf.as_ptr() as *const libc::c_void,
                    buf.len(),
                    flags,
                )
            })
        })
    }));
    Errno::result(res).map(drop)
}

fn removexattr_<P1: ?Sized + NixPath, P2: ?Sized + NixPath>(path: &P1, name: &P2) -> Result<()> {
    let res = try!(try!(unsafe {
        path.with_nix_path(|p| {
            name.with_nix_path(|n| libc::removexattr(p.as_ptr(), n.as_ptr()))
        })
    }));
    Errno::result(res).map(drop)
}

fn lremovexattr<P1: ?Sized + NixPath, P2: ?Sized + NixPath>(path: &P1, name: &P2) -> Result<()> {
    let res = try!(try!(unsafe {
        path.with_nix_path(|p| {
            name.with_nix_path(|n| libc::lremovexattr(p.as_ptr(), n.as_ptr()))
        })
    }));
    Errno::result(res).map(drop)
}

// TODO interesting for rust-nix?
//fn fgetxattr<P: ?Sized + NixPath>(fd: RawFd, name: &P, buf: &mut [u8]) -> Result<usize> {
//    let res = try!(unsafe {
//        name.with_nix_path(|cstr| {
//            libc::fgetxattr(
//                fd,
//                cstr.as_ptr(),
//                buf.as_mut_ptr() as *mut libc::c_void,
//                buf.len(),
//            )
//        })
//    });
//    Errno::result(res).map(|size| size as usize)
//}
//fn flistxattr(fd: RawFd, list: &mut [u8]) -> Result<usize> {
//    let res = unsafe { libc::flistxattr(fd, list.as_mut_ptr() as *mut i8, list.len()) };
//    Errno::result(res).map(|size| size as usize)
//}
//
//fn fsetxattr<P: ?Sized + NixPath>(fd: RawFd, name: &P, buf: &[u8], flags: c_int) -> Result<()> {
//    let res = try!(unsafe {
//        name.with_nix_path(|cstr| {
//            libc::fsetxattr(
//                fd,
//                cstr.as_ptr(),
//                buf.as_ptr() as *const libc::c_void,
//                buf.len(),
//                flags,
//            )
//        })
//    });
//    Errno::result(res).map(drop)
//}
//
//fn fremovexattr<P: ?Sized + NixPath>(fd: RawFd, name: &P) -> Result<()> {
//    let res = try!(unsafe {
//        name.with_nix_path(|cstr| libc::fremovexattr(fd, cstr.as_ptr()))
//    });
//    Errno::result(res).map(drop)
//}
