// SPDX-License-Identifier: MIT
//! Small file-system helpers built on rustix.
//!
//! These replace the parts of `std::fs`/`std::io` that cntr needs, so that
//! file access does not go through libc. They intentionally only cover what
//! callers use; grow them only when a caller needs it.

use rustix::fs::{Mode, OFlags};
use rustix::io::Errno;
use std::os::unix::io::{AsFd, OwnedFd};
use typed_path::{UnixPath, UnixPathBuf};

fn open(path: &UnixPath, flags: OFlags, mode: Mode) -> Result<OwnedFd, Errno> {
    rustix::fs::open(path.as_bytes(), flags | OFlags::CLOEXEC, mode)
}

/// Open a file read-only and return the owned file descriptor.
pub(crate) fn open_read<P: AsRef<UnixPath>>(path: P) -> Result<OwnedFd, Errno> {
    open(path.as_ref(), OFlags::RDONLY, Mode::empty())
}

/// Read the entire file into a byte vector.
pub(crate) fn read<P: AsRef<UnixPath>>(path: P) -> Result<Vec<u8>, Errno> {
    let fd = open_read(path)?;
    let mut contents = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        match rustix::io::read(fd.as_fd(), &mut buf) {
            Ok(0) => return Ok(contents),
            Ok(n) => contents.extend_from_slice(&buf[..n]),
            Err(Errno::INTR) => continue,
            Err(e) => return Err(e),
        }
    }
}

/// Read the entire file into a string.
pub(crate) fn read_to_string<P: AsRef<UnixPath>>(path: P) -> Result<String, Errno> {
    String::from_utf8(read(path)?).map_err(|_| Errno::ILSEQ)
}

/// Write the whole buffer to the given file descriptor, retrying on partial
/// writes and EINTR.
pub(crate) fn write_all<Fd: AsFd>(fd: Fd, mut data: &[u8]) -> Result<(), Errno> {
    while !data.is_empty() {
        match rustix::io::write(fd.as_fd(), data) {
            Ok(0) => return Err(Errno::IO),
            Ok(n) => data = &data[n..],
            Err(Errno::INTR) => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

fn write_flags<P: AsRef<UnixPath>, C: AsRef<[u8]>>(
    path: P,
    contents: C,
    flags: OFlags,
    mode: Mode,
) -> Result<(), Errno> {
    let fd = open(path.as_ref(), OFlags::WRONLY | flags, mode)?;
    write_all(&fd, contents.as_ref())
}

/// Create/truncate a file and write the given contents (like `std::fs::write`).
pub(crate) fn write<P: AsRef<UnixPath>, C: AsRef<[u8]>>(path: P, contents: C) -> Result<(), Errno> {
    write_flags(
        path,
        contents,
        OFlags::CREATE | OFlags::TRUNC,
        Mode::from_raw_mode(0o644),
    )
}

/// Write to an existing file without creating or truncating it
/// (for procfs/sysfs attribute files).
pub(crate) fn write_existing<P: AsRef<UnixPath>, C: AsRef<[u8]>>(
    path: P,
    contents: C,
) -> Result<(), Errno> {
    write_flags(path, contents, OFlags::empty(), Mode::empty())
}

/// Append to an existing file (for cgroup.procs style files).
pub(crate) fn append<P: AsRef<UnixPath>, C: AsRef<[u8]>>(
    path: P,
    contents: C,
) -> Result<(), Errno> {
    write_flags(path, contents, OFlags::APPEND, Mode::empty())
}

/// stat() a path.
pub(crate) fn metadata<P: AsRef<UnixPath>>(path: P) -> Result<rustix::fs::Stat, Errno> {
    rustix::fs::stat(path.as_ref().as_bytes())
}

/// Read the target of a symbolic link.
pub(crate) fn read_link<P: AsRef<UnixPath>>(path: P) -> Result<UnixPathBuf, Errno> {
    let target = rustix::fs::readlink(path.as_ref().as_bytes(), Vec::new())?;
    Ok(UnixPathBuf::from(target.into_bytes()))
}

/// Return the file names contained in a directory (excluding `.` and `..`).
pub(crate) fn read_dir_names<P: AsRef<UnixPath>>(path: P) -> Result<Vec<Vec<u8>>, Errno> {
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
        names.push(name.to_vec());
    }
    Ok(names)
}

/// Recursively create a directory and all of its parents (like
/// `std::fs::create_dir_all`).
pub(crate) fn create_dir_all<P: AsRef<UnixPath>>(path: P) -> Result<(), Errno> {
    let path = path.as_ref();
    match rustix::fs::mkdir(path.as_bytes(), Mode::from_raw_mode(0o755)) {
        Ok(()) => Ok(()),
        Err(Errno::EXIST) => Ok(()),
        Err(Errno::NOENT) => {
            let parent = path.parent().ok_or(Errno::NOENT)?;
            create_dir_all(parent)?;
            match rustix::fs::mkdir(path.as_bytes(), Mode::from_raw_mode(0o755)) {
                Ok(()) | Err(Errno::EXIST) => Ok(()),
                Err(e) => Err(e),
            }
        }
        Err(e) => Err(e),
    }
}
