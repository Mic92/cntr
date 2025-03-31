use libc::{self, c_int, c_ulong};
use nix::errno::Errno;
use simple_error::try_with;
use std::fs::File;
use std::io::Read;
use std::mem;
use std::mem::MaybeUninit;
use std::path::Path;
use std::ptr;
use std::slice;

use crate::procfs;
use crate::result::Result;
use crate::sys_ext::{prctl, setxattr};

pub const _LINUX_CAPABILITY_VERSION_1: u32 = 0x1998_0330;
pub const _LINUX_CAPABILITY_VERSION_2: u32 = 0x2007_1026;
pub const _LINUX_CAPABILITY_VERSION_3: u32 = 0x2008_0522;

pub const VFS_CAP_REVISION_1: u32 = 0x0100_0000;
pub const VFS_CAP_REVISION_2: u32 = 0x0200_0000;
pub const VFS_CAP_REVISION_MASK: u32 = 0xFF00_0000;
pub const VFS_CAP_FLAGS_EFFECTIVE: u32 = 0x00_0001;

pub const CAP_SYS_CHROOT: u32 = 18;
pub const CAP_SYS_PTRACE: u32 = 19;

#[repr(C)]
struct cap_user_header_t {
    version: u32,
    pid: c_int,
}

#[repr(C)]
struct cap_user_data_t {
    effective: u32,
    permitted: u32,
    inheritable: u32,
}

#[repr(C)]
struct _vfs_cap_data {
    permitted: u32,
    inheritable: u32,
}

#[repr(C)]
struct vfs_cap_data {
    magic_etc: u32,
    data: [_vfs_cap_data; 2],
    effective: [u32; 2],
    version: i8,
}

pub fn has_chroot() -> Result<bool> {
    let status = try_with!(
        procfs::status(nix::unistd::getpid()),
        "Failed to get capabilities"
    );
    Ok(status.effective_capabilities & (1 << CAP_SYS_CHROOT) > 0)
}

pub fn set_chroot_capability(path: &Path) -> Result<()> {
    let header: MaybeUninit<cap_user_header_t> = mem::MaybeUninit::uninit();
    let res = unsafe {
        libc::syscall(
            libc::SYS_capget,
            &header,
            ptr::null::<*const cap_user_data_t>(),
        )
    };
    let header: cap_user_header_t = unsafe { header.assume_init() };
    try_with!(Errno::result(res), "Failed to get capability version");

    let (magic, size) = match u32::from_le(header.version) | VFS_CAP_REVISION_MASK {
        _LINUX_CAPABILITY_VERSION_1 => (VFS_CAP_REVISION_1, 4 * (1 + 2)),
        // at the moment _LINUX_CAPABILITY_VERSION_2|_LINUX_CAPABILITY_VERSION_3
        _ => (VFS_CAP_REVISION_2, 4 * (1 + 2 * 2)),
    };

    let data = vfs_cap_data {
        magic_etc: u32::to_le(magic | VFS_CAP_FLAGS_EFFECTIVE),
        data: [
            (_vfs_cap_data {
                permitted: 1 << CAP_SYS_CHROOT,
                inheritable: 0,
            }),
            (_vfs_cap_data {
                permitted: 0,
                inheritable: 0,
            }),
        ],
        effective: [1 << CAP_SYS_CHROOT, 0],
        version: 0,
    };

    let datap: *const vfs_cap_data = &data;
    let bytep: *const u8 = datap as *const _;
    let bytes: &[u8] = unsafe { slice::from_raw_parts(bytep, size) };

    try_with!(
        setxattr(path, "security.capability", bytes, 0),
        "setxattr failed"
    );

    Ok(())
}

fn last_capability() -> Result<c_ulong> {
    let path = "/proc/sys/kernel/cap_last_cap";
    let mut f = try_with!(File::open(path), "failed to open {}", path);

    let mut contents = String::new();
    try_with!(f.read_to_string(&mut contents), "failed to read {}", path);
    contents.pop(); // remove newline
    Ok(try_with!(
        contents.parse::<c_ulong>(),
        "failed to parse capability, got: '{}'",
        contents
    ))
}

pub fn drop(inheritable_capabilities: c_ulong) -> Result<()> {
    // we need chroot at the moment for `exec` command
    let inheritable = inheritable_capabilities | (1 << CAP_SYS_CHROOT) | (1 << CAP_SYS_PTRACE);
    let last_capability = try_with!(last_capability(), "failed to read capability limit");

    for cap in 0..last_capability {
        if (inheritable & (1 << cap)) == 0 {
            // TODO: do not ignore result
            let _ = prctl(libc::PR_CAPBSET_DROP, cap, 0, 0, 0);
        }
    }
    Ok(())
}
