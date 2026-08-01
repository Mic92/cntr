// SPDX-License-Identifier: MIT
//! mount_setattr() wrapper for idmapped mounts
//!
//! rustix has no wrapper for mount_setattr() (kernel 5.12+) yet, so this is
//! the one remaining raw syscall made through libc::syscall().

use std::os::unix::io::{AsRawFd, BorrowedFd};

const AT_EMPTY_PATH: u32 = 0x00001000;
const AT_RECURSIVE: u32 = 0x00008000;
const MOUNT_ATTR_IDMAP: u64 = 0x00100000;

// asm-generic syscall number, used by all architectures except MIPS.
#[cfg(any(target_arch = "mips", target_arch = "mips64"))]
compile_error!("mount_setattr syscall number for MIPS is not defined");
const SYS_MOUNT_SETATTR: libc::c_long = 442;

/// Kernel struct for mount_setattr
#[repr(C)]
struct MountAttr {
    attr_set: u64,
    attr_clr: u64,
    propagation: u64,
    userns_fd: u64,
}

/// Turn a detached mount tree (from open_tree()) into an idmapped mount.
///
/// Files in the mount then appear with ownership translated through the
/// UID/GID mapping of the given user namespace.
pub(crate) fn make_mount_idmapped(
    mount_fd: BorrowedFd,
    userns_fd: BorrowedFd,
) -> std::io::Result<()> {
    let attr = MountAttr {
        attr_set: MOUNT_ATTR_IDMAP,
        attr_clr: 0,
        propagation: 0,
        userns_fd: userns_fd.as_raw_fd() as u64,
    };

    let ret = unsafe {
        libc::syscall(
            SYS_MOUNT_SETATTR,
            mount_fd.as_raw_fd(),
            c"".as_ptr(),
            AT_EMPTY_PATH | AT_RECURSIVE,
            &attr as *const MountAttr,
            std::mem::size_of::<MountAttr>(),
        )
    };
    if ret < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}
