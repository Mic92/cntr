// SPDX-License-Identifier: MIT
//! Kernel capability detection for mount API syscalls
//!
//! This module detects whether the Linux kernel supports the new mount API syscalls
//! by probing at runtime. This is necessary because:
//! 1. Kernel version doesn't guarantee feature availability (distro patches)
//! 2. Syscall numbers may vary by architecture
//! 3. SELinux/seccomp policies may block syscalls

use std::sync::Once;
use std::ffi::CString;

static INIT: Once = Once::new();
static mut MOUNT_API_AVAILABLE: bool = false;

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
    unsafe {
        INIT.call_once(|| {
            MOUNT_API_AVAILABLE = probe_mount_api();
        });
        MOUNT_API_AVAILABLE
    }
}

/// Probe the kernel for mount API support
///
/// Attempts fsopen() with a deliberately invalid filesystem name.
/// - ENOSYS = syscall not implemented → mount API unavailable
/// - ENODEV = device not found → mount API available, just bad fs name
/// - Any other error = assume mount API is available
fn probe_mount_api() -> bool {
    // Use a deliberately non-existent filesystem type to probe
    let probe_fs = CString::new("__cntr_probe__").expect("CString::new failed");

    // Use architecture-specific syscall numbers from libc
    // MIPS uses different offsets: o32=4430, n64=5430, n32=6430
    // Other architectures typically use 430
    unsafe {
        let fd = libc::syscall(libc::SYS_fsopen, probe_fs.as_ptr(), 0) as libc::c_int;

        if fd >= 0 {
            libc::close(fd);
            return true;
        }

        // Check errno to determine if syscall exists
        let errno = *libc::__errno_location();
        match errno {
            libc::ENOSYS => false,              // Syscall not implemented
            libc::ENODEV => true,               // Device/fs not found - syscall exists!
            libc::EPERM | libc::EACCES => true, // Permission denied - syscall exists
            _ => true,                          // Any other error - assume syscall exists
        }
    }
}
