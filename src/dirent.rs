//! Directory Stream functions
//!
//! [Further reading and details on the C API](http://man7.org/linux/man-pages/man3/opendir.3.html)

use libc::{self, c_long, DIR};
use nix;
use nix::errno::Errno;
use std::convert::{AsRef, Into};
use std::mem;

#[cfg(any(target_os = "linux", target_os = "android"))]
use libc::{dirent64, readdir64};

#[cfg(not(any(target_os = "linux", target_os = "android")))]
use libc::{dirent as dirent64, readdir as readdir64};

#[cfg(not(any(target_os = "ios", target_os = "macos")))]
use std::os::unix::io::RawFd;

/// Directory Stream object
#[allow(missing_debug_implementations)]
pub struct DirectoryStream(*mut DIR);

impl AsRef<DIR> for DirectoryStream {
    fn as_ref(&self) -> &DIR {
        unsafe { &*self.0 }
    }
}

/// Consumes directory stream and return underlying directory pointer.
///
/// The pointer must be deallocated manually using `libc::closedir`
impl Into<*mut DIR> for DirectoryStream {
    fn into(self) -> *mut DIR {
        let dirp = self.0;
        mem::forget(self);
        dirp
    }
}

impl Drop for DirectoryStream {
    fn drop(&mut self) {
        unsafe { libc::closedir(self.0) };
    }
}

/// A directory entry
#[allow(missing_debug_implementations)]
pub struct DirectoryEntry<'a>(&'a dirent64);

impl<'a> AsRef<dirent64> for DirectoryEntry<'a> {
    fn as_ref(&self) -> &dirent64 {
        self.0
    }
}

/// Opens a directory stream corresponding to the directory name.
///
/// The stream is positioned at the first entry in the directory.
pub fn opendir<P: ?Sized + nix::NixPath>(name: &P) -> nix::Result<DirectoryStream> {
    let dirp = name.with_nix_path(|cstr| unsafe { libc::opendir(cstr.as_ptr()) })?;
    if dirp.is_null() {
        Err(nix::Error::last().into())
    } else {
        Ok(DirectoryStream(dirp))
    }
}

/// Returns the next directory entry in the directory stream.
///
/// It returns `Some(None)` on reaching the end of the directory stream.
pub fn readdir<'a>(dir: &'a mut DirectoryStream) -> nix::Result<Option<DirectoryEntry>> {
    let dirent = unsafe {
        Errno::clear();
        readdir64(dir.0)
    };
    if dirent.is_null() {
        match Errno::last() {
            Errno::UnknownErrno => Ok(None),
            _ => Err(nix::Error::last().into()),
        }
    } else {
        Ok(Some(DirectoryEntry(unsafe { &*dirent })))
    }
}

/// Sets the location in the directory stream from which the next `readdir` call will start.
///
/// The `loc` argument should be a value returned by a previous call to `telldir`
#[cfg(not(any(target_os = "android")))]
pub fn seekdir<'a>(dir: &'a mut DirectoryStream, loc: c_long) {
    unsafe { libc::seekdir(dir.0, loc) };
}

/// Returns the current location associated with the directory stream.
#[cfg(not(any(target_os = "android")))]
pub fn telldir<'a>(dir: &'a mut DirectoryStream) -> c_long {
    unsafe { libc::telldir(dir.0) }
}

pub fn dirfd<'a>(dir: &'a mut DirectoryStream) -> nix::Result<RawFd> {
    let res = unsafe { libc::dirfd(dir.0) };
    Errno::result(res)
}
