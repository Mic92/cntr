use rustix::thread::{CapabilitySet, remove_capability_from_bounding_set};

pub(crate) const CAP_SYS_CHROOT: u32 = 18;
pub(crate) const CAP_SYS_PTRACE: u32 = 19;

pub(crate) fn drop(inheritable_capabilities: u64, last_cap: u64) {
    // Ensure last_cap won't cause shift overflow
    let max_cap = (u64::BITS - 1) as u64;
    assert!(
        last_cap <= max_cap,
        "last_cap ({}) exceeds maximum bit position ({})",
        last_cap,
        max_cap
    );

    // we need chroot at the moment for `exec` command
    let inheritable =
        inheritable_capabilities | (1u64 << CAP_SYS_CHROOT) | (1u64 << CAP_SYS_PTRACE);

    for cap in 0..=last_cap {
        if (inheritable & (1u64 << cap)) == 0 {
            // Ignore errors - in some contexts we can't drop capabilities (e.g., unprivileged user namespaces)
            // This is expected behavior and should not cause the operation to fail
            let _ =
                remove_capability_from_bounding_set(CapabilitySet::from_bits_retain(1u64 << cap));
        }
    }
}
