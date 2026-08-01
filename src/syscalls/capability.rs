// SPDX-License-Identifier: MIT
//! Kernel capability detection for mount API syscalls
//!
//! This module detects whether the Linux kernel supports the new mount API syscalls
//! by probing at runtime. This is necessary because:
//! 1. Kernel version doesn't guarantee feature availability (distro patches)
//! 2. Syscall numbers may vary by architecture
//! 3. SELinux/seccomp policies may block syscalls

use core::sync::atomic::{AtomicU8, Ordering};
use rustix::io::Errno;
use rustix::mount::{FsOpenFlags, fsopen};

// 0 = not probed yet, 1 = unavailable, 2 = available
static MOUNT_API_AVAILABLE: AtomicU8 = AtomicU8::new(0);

/// Checks if the mount API syscalls are available on this system
///
/// This function probes the kernel by attempting to call `fsopen()` with
/// an invalid filesystem type. If we get ENOSYS, the syscall isn't available.
/// Any other error (like ENODEV) means the syscall exists.
///
/// The result is cached after the first call.
///
/// # Returns
/// * `true` if mount API syscalls are available
/// * `false` if not available (ENOSYS)
pub fn has_mount_api() -> bool {
    match MOUNT_API_AVAILABLE.load(Ordering::Relaxed) {
        0 => {
            let available = probe_mount_api();
            MOUNT_API_AVAILABLE.store(if available { 2 } else { 1 }, Ordering::Relaxed);
            available
        }
        1 => false,
        _ => true,
    }
}

/// Probe the kernel for mount API support
///
/// Attempts fsopen() with a deliberately invalid filesystem name.
/// - ENOSYS = syscall not implemented → mount API unavailable
/// - ENODEV = device not found → mount API available, just bad fs name
/// - Any other error = assume mount API is available
fn probe_mount_api() -> bool {
    match fsopen("__cntr_probe__", FsOpenFlags::empty()) {
        Ok(_) => true,
        Err(Errno::NOSYS) => false, // Syscall not implemented
        // Any other error (ENODEV, EPERM, EACCES, ...) - the syscall exists
        Err(_) => true,
    }
}
