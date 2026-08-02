// SPDX-License-Identifier: MIT
//! mount_setattr() wrapper for idmapped mounts
//!
//! rustix has no wrapper for mount_setattr() (kernel 5.12+) yet, so the
//! syscall is made directly via libc::syscall.

use libc::{AT_EMPTY_PATH, AT_RECURSIVE, MOUNT_ATTR_IDMAP, SYS_mount_setattr, mount_attr};
use std::os::unix::io::{AsRawFd, BorrowedFd};

/// Turn a detached mount tree (from open_tree()) into an idmapped mount.
///
/// Files in the mount then appear with ownership translated through the
/// UID/GID mapping of the given user namespace.
pub(crate) fn make_mount_idmapped(
    mount_fd: BorrowedFd,
    userns_fd: BorrowedFd,
) -> std::io::Result<()> {
    let attr = mount_attr {
        attr_set: MOUNT_ATTR_IDMAP,
        attr_clr: 0,
        propagation: 0,
        userns_fd: userns_fd.as_raw_fd() as u64,
    };

    // SAFETY: attr points to a properly initialized mount_attr struct and the
    // empty path C string outlives the call.
    let res = unsafe {
        libc::syscall(
            SYS_mount_setattr,
            mount_fd.as_raw_fd(),
            c"".as_ptr(),
            AT_EMPTY_PATH | AT_RECURSIVE,
            &raw const attr,
            size_of::<mount_attr>(),
        )
    };
    if res == -1 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}
