// SPDX-License-Identifier: MIT
//! Linux mount API syscall wrappers
//!
//! This module provides FFI wrappers for the new Linux mount API syscalls
//! introduced in kernel 5.2+.
//!
//! These syscalls enable FUSE-free filesystem operations across namespaces.

use std::ffi::CStr;
use std::os::unix::io::{AsRawFd, BorrowedFd, FromRawFd, OwnedFd, RawFd};

// Syscall numbers are consistent across all Linux architectures
// See: include/uapi/asm-generic/unistd.h in kernel source
#[cfg(any(
    target_arch = "x86_64",
    target_arch = "x86",
    target_arch = "aarch64",
    target_arch = "arm",
    target_arch = "riscv64",
    target_arch = "powerpc64",
    target_arch = "powerpc",
    target_arch = "s390x",
    target_arch = "mips64",
    target_arch = "sparc64",
    target_arch = "loongarch64"
))]
mod syscall_numbers {
    pub(crate) const SYS_OPEN_TREE: libc::c_long = 428;
    pub(crate) const SYS_MOVE_MOUNT: libc::c_long = 429;
    pub(crate) const SYS_MOUNT_SETATTR: libc::c_long = 442;
}

#[cfg(not(any(
    target_arch = "x86_64",
    target_arch = "x86",
    target_arch = "aarch64",
    target_arch = "arm",
    target_arch = "riscv64",
    target_arch = "powerpc64",
    target_arch = "powerpc",
    target_arch = "s390x",
    target_arch = "mips64",
    target_arch = "sparc64",
    target_arch = "loongarch64"
)))]
compile_error!(
    "Mount API syscalls are not supported on this architecture. Please report this issue."
);

use syscall_numbers::*;

// Flags and constants

// open_tree() flags
pub(crate) const OPEN_TREE_CLONE: u32 = 0x00000001;
pub(crate) const AT_RECURSIVE: u32 = 0x00008000;

// move_mount() flags
pub(crate) const MOVE_MOUNT_F_EMPTY_PATH: u32 = 0x00000004;

// mount_setattr() flags
pub(crate) const AT_EMPTY_PATH: u32 = 0x00001000;
pub(crate) const MOUNT_ATTR_IDMAP: u64 = 0x00100000;

// Directory file descriptor constants
pub(crate) const AT_FDCWD: libc::c_int = -100;

/// Kernel struct for mount_setattr
#[repr(C)]
pub(crate) struct MountAttr {
    pub attr_set: u64,
    pub attr_clr: u64,
    pub propagation: u64,
    pub userns_fd: u64,
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

/// Set mount attributes (raw syscall)
///
/// # Arguments
/// * `dfd` - Directory file descriptor (or a mount FD with AT_EMPTY_PATH)
/// * `path` - Path to apply attributes to (or empty string with AT_EMPTY_PATH)
/// * `flags` - Flags (e.g., AT_EMPTY_PATH | AT_RECURSIVE)
/// * `attr` - Mount attributes to set
/// * `size` - Size of attr struct
unsafe fn mount_setattr(
    dfd: RawFd,
    path: *const libc::c_char,
    flags: u32,
    attr: *const MountAttr,
    size: libc::size_t,
) -> libc::c_int {
    unsafe { libc::syscall(SYS_MOUNT_SETATTR, dfd, path, flags, attr, size) as libc::c_int }
}

// Safe wrapper types with RAII semantics

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

    /// Apply an idmapped mount to this mount tree
    ///
    /// # Arguments
    /// * `userns_fd` - File descriptor to user namespace with the desired UID/GID mapping
    ///
    /// This makes files in the mount appear with different ownership based on the
    /// user namespace mapping. Requires kernel 5.12+.
    pub(crate) fn apply_idmap(&self, userns_fd: BorrowedFd) -> Result<(), std::io::Error> {
        let attr = MountAttr {
            attr_set: MOUNT_ATTR_IDMAP,
            attr_clr: 0,
            propagation: 0,
            userns_fd: userns_fd.as_raw_fd() as u64,
        };

        unsafe {
            let empty_path = c"";
            let ret = mount_setattr(
                self.fd.as_raw_fd(),
                empty_path.as_ptr(),
                AT_EMPTY_PATH | AT_RECURSIVE,
                &attr,
                std::mem::size_of::<MountAttr>(),
            );
            if ret < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        }
    }

    /// Move this mount to a target location
    ///
    /// # Arguments
    /// * `to_dfd` - Optional destination directory fd (None = current working directory)
    /// * `to_path` - Destination path
    /// * `flags` - Movement flags
    pub(crate) fn attach_to(
        self,
        to_dfd: Option<BorrowedFd>,
        to_path: &CStr,
        flags: u32,
    ) -> Result<(), std::io::Error> {
        unsafe {
            let empty_path = c"";
            let dfd = to_dfd.map(|fd| fd.as_raw_fd()).unwrap_or(AT_FDCWD);
            let ret = move_mount(
                self.fd.as_raw_fd(),
                empty_path.as_ptr(),
                dfd,
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
