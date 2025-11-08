use libc::c_ulong;
use simple_error::try_with;
use std::fs::File;
use std::io::Read;

use crate::result::Result;
use crate::syscalls::prctl;

pub const CAP_SYS_CHROOT: u32 = 18;
pub const CAP_SYS_PTRACE: u32 = 19;

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
