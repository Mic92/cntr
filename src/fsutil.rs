// SPDX-License-Identifier: MIT
//! Small file-system helpers built on rustix.
//!
//! These replace the parts of `std::fs`/`std::io` that cntr needs, so that
//! file access does not go through libc. They intentionally only cover what
//! callers use; grow them only when a caller needs it.

use rustix::fs::{Mode, OFlags};
use std::ffi::OsString;
use std::io;
use std::os::unix::ffi::OsStringExt;
use std::os::unix::io::{AsFd, OwnedFd};
use std::path::{Path, PathBuf};

fn open(path: &Path, flags: OFlags, mode: Mode) -> io::Result<OwnedFd> {
    rustix::fs::open(path, flags | OFlags::CLOEXEC, mode).map_err(io::Error::from)
}

/// Open a file read-only and return the owned file descriptor.
pub(crate) fn open_read<P: AsRef<Path>>(path: P) -> io::Result<OwnedFd> {
    open(path.as_ref(), OFlags::RDONLY, Mode::empty())
}

/// Read the entire file into a byte vector.
pub(crate) fn read<P: AsRef<Path>>(path: P) -> io::Result<Vec<u8>> {
    let fd = open_read(path)?;
    let mut contents = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        match rustix::io::read(fd.as_fd(), &mut buf) {
            Ok(0) => return Ok(contents),
            Ok(n) => contents.extend_from_slice(&buf[..n]),
            Err(rustix::io::Errno::INTR) => continue,
            Err(e) => return Err(e.into()),
        }
    }
}

/// Read the entire file into a string.
pub(crate) fn read_to_string<P: AsRef<Path>>(path: P) -> io::Result<String> {
    String::from_utf8(read(path)?)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.utf8_error()))
}

fn write_all(fd: &OwnedFd, mut data: &[u8]) -> io::Result<()> {
    while !data.is_empty() {
        match rustix::io::write(fd.as_fd(), data) {
            Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
            Ok(n) => data = &data[n..],
            Err(rustix::io::Errno::INTR) => continue,
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}

fn write_flags<P: AsRef<Path>, C: AsRef<[u8]>>(
    path: P,
    contents: C,
    flags: OFlags,
    mode: Mode,
) -> io::Result<()> {
    let fd = open(path.as_ref(), OFlags::WRONLY | flags, mode)?;
    write_all(&fd, contents.as_ref())
}

/// Create/truncate a file and write the given contents (like `std::fs::write`).
pub(crate) fn write<P: AsRef<Path>, C: AsRef<[u8]>>(path: P, contents: C) -> io::Result<()> {
    write_flags(
        path,
        contents,
        OFlags::CREATE | OFlags::TRUNC,
        Mode::from_raw_mode(0o644),
    )
}

/// Write to an existing file without creating or truncating it
/// (for procfs/sysfs attribute files).
pub(crate) fn write_existing<P: AsRef<Path>, C: AsRef<[u8]>>(
    path: P,
    contents: C,
) -> io::Result<()> {
    write_flags(path, contents, OFlags::empty(), Mode::empty())
}

/// Append to an existing file (for cgroup.procs style files).
pub(crate) fn append<P: AsRef<Path>, C: AsRef<[u8]>>(path: P, contents: C) -> io::Result<()> {
    write_flags(path, contents, OFlags::APPEND, Mode::empty())
}

/// stat() a path.
pub(crate) fn metadata<P: AsRef<Path>>(path: P) -> io::Result<rustix::fs::Stat> {
    rustix::fs::stat(path.as_ref()).map_err(io::Error::from)
}

/// Read the target of a symbolic link.
pub(crate) fn read_link<P: AsRef<Path>>(path: P) -> io::Result<PathBuf> {
    let target = rustix::fs::readlink(path.as_ref(), Vec::new())?;
    Ok(PathBuf::from(OsString::from_vec(target.into_bytes())))
}

/// Return the file names contained in a directory (excluding `.` and `..`).
pub(crate) fn read_dir_names<P: AsRef<Path>>(path: P) -> io::Result<Vec<OsString>> {
    let fd = open(
        path.as_ref(),
        OFlags::RDONLY | OFlags::DIRECTORY,
        Mode::empty(),
    )?;
    let dir = rustix::fs::Dir::read_from(fd.as_fd())?;
    let mut names = Vec::new();
    for entry in dir {
        let entry = entry?;
        let name = entry.file_name().to_bytes();
        if name == b"." || name == b".." {
            continue;
        }
        names.push(OsString::from_vec(name.to_vec()));
    }
    Ok(names)
}

/// Recursively create a directory and all of its parents (like
/// `std::fs::create_dir_all`).
pub(crate) fn create_dir_all<P: AsRef<Path>>(path: P) -> io::Result<()> {
    let path = path.as_ref();
    match rustix::fs::mkdir(path, Mode::from_raw_mode(0o755)) {
        Ok(()) => Ok(()),
        Err(rustix::io::Errno::EXIST) => Ok(()),
        Err(rustix::io::Errno::NOENT) => {
            let parent = path
                .parent()
                .ok_or_else(|| io::Error::from(rustix::io::Errno::NOENT))?;
            create_dir_all(parent)?;
            match rustix::fs::mkdir(path, Mode::from_raw_mode(0o755)) {
                Ok(()) | Err(rustix::io::Errno::EXIST) => Ok(()),
                Err(e) => Err(e.into()),
            }
        }
        Err(e) => Err(e.into()),
    }
}
