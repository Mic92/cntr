use files::Fd;
use fuse::FileType;
use nix::sys::xattr as _xattr;
use nix::Result;
use readlink::readlinkat;
use std::ffi::OsStr;

pub fn setxattr(fd: &Fd, kind: FileType, name: &OsStr, value: &[u8], flags: u32) -> Result<()> {
    if kind == FileType::Symlink {
        let path = readlinkat(fd.raw())?;
        _xattr::lsetxattr(path.as_os_str(), name, value, flags as i32)
    } else {
        _xattr::setxattr(fd.path().as_str(), name, value, flags as i32)
    }
}

pub fn removexattr(fd: &Fd, kind: FileType, name: &OsStr) -> Result<()> {
    if kind == FileType::Symlink {
        let path = readlinkat(fd.raw())?;
        _xattr::lremovexattr(path.as_os_str(), name)
    } else {
        _xattr::removexattr(fd.path().as_str(), name)
    }
}

pub fn listxattr(fd: &Fd, kind: FileType, name: &mut [u8]) -> Result<usize> {
    if kind == FileType::Symlink {
        let path = readlinkat(fd.raw())?;
        _xattr::llistxattr(path.as_os_str(), name)
    } else {
        _xattr::listxattr(fd.path().as_str(), name)
    }
}

pub fn getxattr(fd: &Fd, kind: FileType, name: &OsStr, buf: &mut [u8]) -> Result<usize> {
    if kind == FileType::Symlink {
        let path = readlinkat(fd.raw())?;
        _xattr::lgetxattr(path.as_os_str(), name, buf)
    } else {
        _xattr::getxattr(fd.path().as_str(), name, buf)
    }
}
