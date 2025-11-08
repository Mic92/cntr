use libc::c_ulong;

use crate::result::Result;
use crate::syscalls::prctl;

pub(crate) const CAP_SYS_CHROOT: u32 = 18;
pub(crate) const CAP_SYS_PTRACE: u32 = 19;

pub(crate) fn drop(inheritable_capabilities: c_ulong, last_cap: c_ulong) -> Result<()> {
    // we need chroot at the moment for `exec` command
    let inheritable = inheritable_capabilities | (1 << CAP_SYS_CHROOT) | (1 << CAP_SYS_PTRACE);

    for cap in 0..last_cap {
        if (inheritable & (1 << cap)) == 0 {
            // TODO: do not ignore result
            let _ = prctl(libc::PR_CAPBSET_DROP, cap, 0, 0, 0);
        }
    }
    Ok(())
}
