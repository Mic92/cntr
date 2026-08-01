// SPDX-License-Identifier: MIT
//! Linux mount API helpers
//!
//! Wraps the new Linux mount API (kernel 5.2+) used to attach container
//! mount trees across namespaces and to create idmapped mounts.
//!
//! open_tree() and move_mount() come from rustix. mount_setattr() has no
//! rustix wrapper yet, so it is the one remaining raw syscall made through
//! libc::syscall().

use rustix::fs::CWD;
use rustix::mount::{MoveMountFlags, OpenTreeFlags, move_mount, open_tree};
use std::ffi::CStr;
use std::os::unix::io::{AsFd, AsRawFd, BorrowedFd, OwnedFd};

// mount_setattr() flags
const AT_EMPTY_PATH: u32 = 0x00001000;
const AT_RECURSIVE: u32 = 0x00008000;
const MOUNT_ATTR_IDMAP: u64 = 0x00100000;

// mount_setattr() is not wired up for MIPS-style syscall numbering here;
// the number below is the asm-generic one used by all other architectures.
#[cfg(target_arch = "mips64")]
compile_error!("mount_setattr syscall number for this architecture is not defined");
const SYS_MOUNT_SETATTR: libc::c_long = 442;

/// Kernel struct for mount_setattr
#[repr(C)]
struct MountAttr {
    attr_set: u64,
    attr_clr: u64,
    propagation: u64,
    userns_fd: u64,
}

/// Set mount attributes (raw syscall, no rustix wrapper available yet)
unsafe fn mount_setattr(
    dfd: BorrowedFd,
    path: &CStr,
    flags: u32,
    attr: &MountAttr,
) -> std::io::Result<()> {
    let ret = unsafe {
        libc::syscall(
            SYS_MOUNT_SETATTR,
            dfd.as_raw_fd(),
            path.as_ptr(),
            flags,
            attr as *const MountAttr,
            std::mem::size_of::<MountAttr>(),
        )
    };
    if ret < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

// Safe wrapper types with RAII semantics

/// RAII wrapper for mount file descriptor
///
/// Represents a detached mount created by open_tree().
/// The fd is automatically closed when this struct is dropped.
pub(crate) struct MountFd {
    fd: OwnedFd,
}

impl MountFd {
    /// Create a detached, recursive copy of a mount tree
    ///
    /// # Arguments
    /// * `dirfd` - Optional directory file descriptor to use as base (None = current directory)
    /// * `path` - Path to the mount point (absolute if dirfd is None, relative otherwise)
    pub(crate) fn open_tree_at(
        dirfd: Option<BorrowedFd>,
        path: &CStr,
    ) -> Result<Self, std::io::Error> {
        let dfd = dirfd.unwrap_or(CWD);
        let fd = open_tree(
            dfd,
            path,
            OpenTreeFlags::OPEN_TREE_CLONE | OpenTreeFlags::AT_RECURSIVE,
        )?;
        Ok(MountFd { fd })
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

        unsafe { mount_setattr(self.fd.as_fd(), c"", AT_EMPTY_PATH | AT_RECURSIVE, &attr) }
    }

    /// Move this mount to a target location
    ///
    /// # Arguments
    /// * `to_dfd` - Optional destination directory fd (None = current working directory)
    /// * `to_path` - Destination path
    pub(crate) fn attach_to(
        self,
        to_dfd: Option<BorrowedFd>,
        to_path: &CStr,
    ) -> Result<(), std::io::Error> {
        let dfd = to_dfd.unwrap_or(CWD);
        move_mount(
            self.fd.as_fd(),
            c"",
            dfd,
            to_path,
            MoveMountFlags::MOVE_MOUNT_F_EMPTY_PATH,
        )?;
        Ok(())
    }
}
