// SPDX-License-Identifier: MIT
//! mount_setattr() helper for idmapped mounts

use rustix::fd::{AsRawFd, BorrowedFd};
use rustix::mount::{MountAttr, MountAttrFlags, MountSetattrFlags, mount_setattr};

/// Turn a detached mount tree (from open_tree()) into an idmapped mount.
///
/// Files in the mount then appear with ownership translated through the
/// UID/GID mapping of the given user namespace.
pub(crate) fn make_mount_idmapped(
    mount_fd: BorrowedFd,
    userns_fd: BorrowedFd,
) -> Result<(), rustix::io::Errno> {
    let attr = MountAttr {
        attr_set: u64::from(MountAttrFlags::MOUNT_ATTR_IDMAP.bits()),
        userns_fd: mount_fd_to_u64(userns_fd),
        ..Default::default()
    };
    mount_setattr(
        mount_fd,
        "",
        MountSetattrFlags::AT_EMPTY_PATH | MountSetattrFlags::AT_RECURSIVE,
        &attr,
    )
}

fn mount_fd_to_u64(fd: BorrowedFd) -> u64 {
    fd.as_raw_fd() as u64
}
