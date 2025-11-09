use libc::c_ulong;

use crate::result::Result;
use crate::syscalls::prctl;

pub(crate) const CAP_SYS_CHROOT: u32 = 18;
pub(crate) const CAP_SYS_PTRACE: u32 = 19;

pub(crate) fn drop(inheritable_capabilities: c_ulong, last_cap: c_ulong) -> Result<()> {
    // Ensure last_cap won't cause shift overflow
    let max_cap = (std::mem::size_of::<c_ulong>() * 8 - 1) as c_ulong;
    assert!(
        last_cap <= max_cap,
        "last_cap ({}) exceeds maximum bit position ({})",
        last_cap,
        max_cap
    );

    // we need chroot at the moment for `exec` command
    let inheritable = inheritable_capabilities
        | ((1 as c_ulong) << CAP_SYS_CHROOT)
        | ((1 as c_ulong) << CAP_SYS_PTRACE);

    for cap in 0..=last_cap {
        if (inheritable & ((1 as c_ulong) << cap)) == 0 {
            // TODO: do not ignore result
            let _ = prctl(libc::PR_CAPBSET_DROP, cap, 0, 0, 0);
        }
    }
    Ok(())
}
