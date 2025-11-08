// SPDX-License-Identifier: MIT
//! Kernel capability detection for mount API syscalls
//!
//! This module detects whether the Linux kernel supports the new mount API syscalls
//! by probing at runtime. This is necessary because:
//! 1. Kernel version doesn't guarantee feature availability (distro patches)
//! 2. Syscall numbers may vary by architecture
//! 3. SELinux/seccomp policies may block syscalls

use std::sync::Once;

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
    // fsopen is syscall 430 on all supported Linux architectures
    // For unsupported architectures, assume mount API is not available
    #[cfg(any(
        target_arch = "x86_64",
        target_arch = "x86",
        target_arch = "aarch64",
        target_arch = "arm",
        target_arch = "riscv64",
        target_arch = "powerpc64",
        target_arch = "powerpc",
        target_arch = "s390x",
        target_arch = "mips64",
        target_arch = "sparc64",
        target_arch = "loongarch64"
    ))]
    {
        use std::ffi::CString;

        // Use a deliberately non-existent filesystem type to probe
        let probe_fs = CString::new("__cntr_probe__").expect("CString::new failed");

        // fsopen syscall number is 430 on all Linux architectures
        const SYS_FSOPEN: libc::c_long = 430;

        unsafe {
            let fd = libc::syscall(SYS_FSOPEN, probe_fs.as_ptr(), 0) as libc::c_int;

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

    #[cfg(not(any(
        target_arch = "x86_64",
        target_arch = "x86",
        target_arch = "aarch64",
        target_arch = "arm",
        target_arch = "riscv64",
        target_arch = "powerpc64",
        target_arch = "powerpc",
        target_arch = "s390x",
        target_arch = "mips64",
        target_arch = "sparc64",
        target_arch = "loongarch64"
    )))]
    {
        // For unsupported architectures, conservatively assume mount API is not available
        false
    }
}
