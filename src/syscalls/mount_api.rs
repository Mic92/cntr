// SPDX-License-Identifier: MIT
//! mount_setattr() wrapper for idmapped mounts
//!
//! rustix has no wrapper for mount_setattr() (kernel 5.12+) yet, so the
//! syscall is made directly: the number and struct come from linux-raw-sys
//! (the kernel UAPI bindings rustix itself uses) and the syscall instruction
//! from the syscalls crate.

use linux_raw_sys::general::{AT_EMPTY_PATH, AT_RECURSIVE, MOUNT_ATTR_IDMAP, mount_attr};
use std::os::unix::io::{AsRawFd, BorrowedFd};
use syscalls::{Sysno, syscall};

/// Turn a detached mount tree (from open_tree()) into an idmapped mount.
///
/// Files in the mount then appear with ownership translated through the
/// UID/GID mapping of the given user namespace.
pub(crate) fn make_mount_idmapped(
    mount_fd: BorrowedFd,
    userns_fd: BorrowedFd,
) -> Result<(), rustix::io::Errno> {
    let attr = mount_attr {
        attr_set: u64::from(MOUNT_ATTR_IDMAP),
        attr_clr: 0,
        propagation: 0,
        userns_fd: userns_fd.as_raw_fd() as u64,
    };

    // SAFETY: attr points to a properly initialized mount_attr struct and the
    // empty path C string outlives the call.
    let res = unsafe {
        syscall!(
            Sysno::mount_setattr,
            mount_fd.as_raw_fd(),
            c"".as_ptr(),
            AT_EMPTY_PATH | AT_RECURSIVE,
            &raw const attr,
            size_of::<mount_attr>()
        )
    };
    res.map(|_| ())
        .map_err(|e| rustix::io::Errno::from_raw_os_error(e.into_raw()))
}
