use anyhow::Context;
use libc::c_ulong;
use std::fs::File;
use std::io::Read;

use crate::result::Result;
use crate::syscalls::prctl;

pub(crate) const CAP_SYS_CHROOT: u32 = 18;
pub(crate) const CAP_SYS_PTRACE: u32 = 19;

fn last_capability() -> Result<c_ulong> {
    let path = "/proc/sys/kernel/cap_last_cap";
    let mut f = File::open(path).with_context(|| format!("failed to open {}", path))?;

    let mut contents = String::new();
    f.read_to_string(&mut contents)
        .with_context(|| format!("failed to read {}", path))?;
    contents.pop(); // remove newline
    contents.parse::<c_ulong>().with_context(|| {
        format!(
            "failed to parse last capability value from {}: '{}'",
            path, contents
        )
    })
}

pub(crate) fn drop(inheritable_capabilities: c_ulong) -> Result<()> {
    // we need chroot at the moment for `exec` command
    let inheritable = inheritable_capabilities | (1 << CAP_SYS_CHROOT) | (1 << CAP_SYS_PTRACE);
    let last_capability =
        last_capability().context("failed to read capability limit from /proc")?;

    for cap in 0..last_capability {
        if (inheritable & (1 << cap)) == 0 {
            // TODO: do not ignore result
            let _ = prctl(libc::PR_CAPBSET_DROP, cap, 0, 0, 0);
        }
    }
    Ok(())
}
