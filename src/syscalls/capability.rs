// SPDX-License-Identifier: MIT
//! Kernel capability detection for mount API syscalls
//!
//! This module detects whether the Linux kernel supports the new mount API syscalls
//! by probing at runtime. This is necessary because:
//! 1. Kernel version doesn't guarantee feature availability (distro patches)
//! 2. Syscall numbers may vary by architecture
//! 3. SELinux/seccomp policies may block syscalls

use std::ffi::CString;
use std::sync::Once;

use super::mount_api;

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

    match mount_api::FsContext::open(&probe_fs, 0) {
        Ok(_ctx) => {
            // Unexpectedly succeeded - but this means the API exists
            true
        }
        Err(e) => {
            // Check the error to determine if syscall exists
            match e.raw_os_error() {
                Some(libc::ENOSYS) => {
                    // Syscall not implemented
                    false
                }
                Some(libc::ENODEV) => {
                    // Device/filesystem not found - syscall exists!
                    true
                }
                Some(libc::EPERM) | Some(libc::EACCES) => {
                    // Permission denied - syscall exists but we lack permissions
                    // This is common in containers with restricted capabilities
                    true
                }
                _ => {
                    // Any other error - assume syscall exists
                    true
                }
            }
        }
    }
}

/// Require mount API support or exit with an error
///
/// This function checks for mount API support and terminates the program
/// with a clear error message if the required syscalls are not available.
///
/// # Panics
/// Panics if the mount API is not available on this kernel
pub fn require_mount_api() {
    if !has_mount_api() {
        eprintln!("ERROR: This kernel does not support the mount API syscalls.");
        eprintln!("Required: Linux 5.2+ for fsopen/fsmount/move_mount");
        eprintln!("          Linux 6.8+ for statmount/listmount");
        eprintln!();
        eprintln!("Your kernel version:");
        if let Ok(version) = std::fs::read_to_string("/proc/sys/kernel/osrelease") {
            eprintln!("  {}", version.trim());
        }
        eprintln!();
        eprintln!("cntr requires a modern kernel with mount API support.");
        eprintln!("Please upgrade your kernel or use an older version of cntr with FUSE support.");
        std::process::exit(1);
    }
}
