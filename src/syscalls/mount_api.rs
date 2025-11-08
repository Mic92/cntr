// SPDX-License-Identifier: MIT
//! Linux mount API syscall wrappers
//!
//! This module provides FFI wrappers for the new Linux mount API syscalls
//! introduced in kernel 5.2+.
//!
//! These syscalls enable FUSE-free filesystem operations across namespaces.

use std::ffi::CStr;
use std::os::unix::io::{AsRawFd, FromRawFd, OwnedFd, RawFd};

// Syscall numbers for x86_64 (asm-generic would be the same for most architectures)
// See: include/uapi/asm-generic/unistd.h in kernel source
#[cfg(target_arch = "x86_64")]
mod syscall_numbers {
    pub(crate) const SYS_OPEN_TREE: libc::c_long = 428;
    pub(crate) const SYS_MOVE_MOUNT: libc::c_long = 429;
    pub(crate) const SYS_FSOPEN: libc::c_long = 430;
}

#[cfg(target_arch = "aarch64")]
mod syscall_numbers {
    pub(crate) const SYS_OPEN_TREE: libc::c_long = 428;
    pub(crate) const SYS_MOVE_MOUNT: libc::c_long = 429;
    pub(crate) const SYS_FSOPEN: libc::c_long = 430;
}

use syscall_numbers::*;

// Flags and constants

// open_tree() flags
pub(crate) const OPEN_TREE_CLONE: u32 = 0x00000001;
pub(crate) const AT_RECURSIVE: u32 = 0x00008000;

// move_mount() flags
pub(crate) const MOVE_MOUNT_F_EMPTY_PATH: u32 = 0x00000004;

// Directory file descriptor constants
pub(crate) const AT_FDCWD: libc::c_int = -100;

/// Open a filesystem configuration context (raw syscall)
///
/// # Arguments
/// * `fs_name` - Filesystem type (e.g., "tmpfs", "ext4")
/// * `flags` - Flags for the operation
///
/// # Returns
/// File descriptor for the filesystem context on success, or -1 on error
unsafe fn fsopen(fs_name: &CStr, flags: u32) -> RawFd {
    unsafe { libc::syscall(SYS_FSOPEN, fs_name.as_ptr(), flags) as RawFd }
}

/// Move a mount from one place to another (raw syscall)
unsafe fn move_mount(
    from_dfd: RawFd,
    from_path: *const libc::c_char,
    to_dfd: RawFd,
    to_path: *const libc::c_char,
    flags: u32,
) -> libc::c_int {
    unsafe {
        libc::syscall(SYS_MOVE_MOUNT, from_dfd, from_path, to_dfd, to_path, flags) as libc::c_int
    }
}

/// Open a mount tree and return a detached mount FD (raw syscall)
///
/// # Arguments
/// * `dfd` - Directory file descriptor (or AT_FDCWD)
/// * `filename` - Path to open
/// * `flags` - Flags (e.g., OPEN_TREE_CLONE | AT_RECURSIVE)
unsafe fn open_tree(dfd: RawFd, filename: *const libc::c_char, flags: u32) -> RawFd {
    unsafe { libc::syscall(SYS_OPEN_TREE, dfd, filename, flags) as RawFd }
}

// Safe wrapper types with RAII semantics

/// RAII wrapper for filesystem configuration context
///
/// Represents an open filesystem configuration created by fsopen().
/// The fd is automatically closed when this struct is dropped.
pub(crate) struct FsContext {
    fd: OwnedFd,
}

impl FsContext {
    /// Open a filesystem configuration context
    ///
    /// # Arguments
    /// * `fs_name` - Filesystem type (e.g., "tmpfs")
    /// * `flags` - Flags
    pub(crate) fn open(fs_name: &CStr, flags: u32) -> Result<Self, std::io::Error> {
        unsafe {
            let fd = fsopen(fs_name, flags);
            if fd < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(FsContext {
                fd: OwnedFd::from_raw_fd(fd),
            })
        }
    }
}

impl AsRawFd for FsContext {
    fn as_raw_fd(&self) -> RawFd {
        self.fd.as_raw_fd()
    }
}

/// RAII wrapper for mount file descriptor
///
/// Represents a detached mount created by fsmount() or open_tree().
/// The fd is automatically closed when this struct is dropped.
pub(crate) struct MountFd {
    fd: OwnedFd,
}

impl MountFd {
    /// Create a detached copy of a mount tree using AT_FDCWD
    ///
    /// # Arguments
    /// * `path` - Path to the mount point
    /// * `flags` - Flags (typically OPEN_TREE_CLONE | AT_RECURSIVE)
    pub(crate) fn open_tree_at(path: &CStr, flags: u32) -> Result<Self, std::io::Error> {
        unsafe {
            let mount_fd = open_tree(AT_FDCWD, path.as_ptr(), flags);
            if mount_fd < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(MountFd {
                fd: OwnedFd::from_raw_fd(mount_fd),
            })
        }
    }

    /// Move this mount to a target location
    ///
    /// # Arguments
    /// * `to_dfd` - Destination directory fd (or AT_FDCWD)
    /// * `to_path` - Destination path
    /// * `flags` - Movement flags
    pub(crate) fn attach_to(
        self,
        to_dfd: RawFd,
        to_path: &CStr,
        flags: u32,
    ) -> Result<(), std::io::Error> {
        unsafe {
            let empty_path = c"";
            let ret = move_mount(
                self.fd.as_raw_fd(),
                empty_path.as_ptr(),
                to_dfd,
                to_path.as_ptr(),
                flags | MOVE_MOUNT_F_EMPTY_PATH,
            );
            if ret < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        }
    }
}

impl AsRawFd for MountFd {
    fn as_raw_fd(&self) -> RawFd {
        self.fd.as_raw_fd()
    }
}
