// SPDX-License-Identifier: MIT
//! Linux mount API syscall wrappers
//!
//! This module provides FFI wrappers for the new Linux mount API syscalls
//! introduced in kernel 5.2+.
//!
//! These syscalls enable FUSE-free filesystem operations across namespaces.

use std::ffi::CStr;
use std::os::unix::io::{AsFd, AsRawFd, FromRawFd, OwnedFd, RawFd};

// Syscall numbers for x86_64 (asm-generic would be the same for most architectures)
// See: include/uapi/asm-generic/unistd.h in kernel source
#[cfg(target_arch = "x86_64")]
mod syscall_numbers {
    pub const SYS_OPEN_TREE: libc::c_long = 428;
    pub const SYS_MOVE_MOUNT: libc::c_long = 429;
    pub const SYS_FSOPEN: libc::c_long = 430;
    pub const SYS_FSCONFIG: libc::c_long = 431;
    pub const SYS_FSMOUNT: libc::c_long = 432;
}

#[cfg(target_arch = "aarch64")]
mod syscall_numbers {
    pub const SYS_OPEN_TREE: libc::c_long = 428;
    pub const SYS_MOVE_MOUNT: libc::c_long = 429;
    pub const SYS_FSOPEN: libc::c_long = 430;
    pub const SYS_FSCONFIG: libc::c_long = 431;
    pub const SYS_FSMOUNT: libc::c_long = 432;
}

use syscall_numbers::*;

// Flags and constants

// open_tree() flags
pub const OPEN_TREE_CLONE: u32 = 0x00000001;
pub const AT_RECURSIVE: u32 = 0x00008000;

// fsopen() flags
pub const FSOPEN_CLOEXEC: u32 = 0x000001;

// fsmount() flags
pub const FSMOUNT_CLOEXEC: u32 = 0x000001;

// fsconfig() commands
pub const FSCONFIG_SET_FLAG: u32 = 0;
pub const FSCONFIG_SET_STRING: u32 = 1;
pub const FSCONFIG_SET_BINARY: u32 = 2;
pub const FSCONFIG_SET_PATH: u32 = 3;
pub const FSCONFIG_SET_PATH_EMPTY: u32 = 4;
pub const FSCONFIG_SET_FD: u32 = 5;
pub const FSCONFIG_CMD_CREATE: u32 = 6;
pub const FSCONFIG_CMD_RECONFIGURE: u32 = 7;

// move_mount() flags
pub const MOVE_MOUNT_F_SYMLINKS: u32 = 0x00000001;
pub const MOVE_MOUNT_F_AUTOMOUNTS: u32 = 0x00000002;
pub const MOVE_MOUNT_F_EMPTY_PATH: u32 = 0x00000004;
pub const MOVE_MOUNT_T_SYMLINKS: u32 = 0x00000010;
pub const MOVE_MOUNT_T_AUTOMOUNTS: u32 = 0x00000020;
pub const MOVE_MOUNT_T_EMPTY_PATH: u32 = 0x00000040;
pub const MOVE_MOUNT__MASK: u32 = 0x00000077;

// Directory file descriptor constants
pub const AT_FDCWD: libc::c_int = -100;

/// Open a filesystem configuration context (raw syscall)
///
/// # Arguments
/// * `fs_name` - Filesystem type (e.g., "tmpfs", "ext4")
/// * `flags` - Flags for the operation (e.g., FSOPEN_CLOEXEC)
///
/// # Returns
/// File descriptor for the filesystem context on success, or -1 on error
unsafe fn fsopen(fs_name: &CStr, flags: u32) -> RawFd {
    unsafe { libc::syscall(SYS_FSOPEN, fs_name.as_ptr(), flags) as RawFd }
}

/// Configure a filesystem creation context (raw syscall)
unsafe fn fsconfig(
    fd: RawFd,
    cmd: u32,
    key: *const libc::c_char,
    value: *const libc::c_void,
    aux: libc::c_int,
) -> libc::c_int {
    unsafe { libc::syscall(SYS_FSCONFIG, fd, cmd, key, value, aux) as libc::c_int }
}

/// Create a detached mount from a filesystem context (raw syscall)
unsafe fn fsmount(fs_fd: RawFd, flags: u32, attr_flags: u32) -> RawFd {
    unsafe { libc::syscall(SYS_FSMOUNT, fs_fd, flags, attr_flags) as RawFd }
}

/// Move a mount from one place to another (raw syscall)
unsafe fn move_mount(
    from_dfd: RawFd,
    from_path: *const libc::c_char,
    to_dfd: RawFd,
    to_path: *const libc::c_char,
    flags: u32,
) -> libc::c_int {
    unsafe {
        libc::syscall(SYS_MOVE_MOUNT, from_dfd, from_path, to_dfd, to_path, flags) as libc::c_int
    }
}

/// Open a mount tree and return a detached mount FD (raw syscall)
///
/// # Arguments
/// * `dfd` - Directory file descriptor (or AT_FDCWD)
/// * `filename` - Path to open
/// * `flags` - Flags (e.g., OPEN_TREE_CLONE | AT_RECURSIVE)
unsafe fn open_tree(dfd: RawFd, filename: *const libc::c_char, flags: u32) -> RawFd {
    unsafe { libc::syscall(SYS_OPEN_TREE, dfd, filename, flags) as RawFd }
}

// Safe wrapper types with RAII semantics

/// RAII wrapper for filesystem configuration context
///
/// Represents an open filesystem configuration created by fsopen().
/// The fd is automatically closed when this struct is dropped.
pub struct FsContext {
    fd: OwnedFd,
}

impl FsContext {
    /// Open a filesystem configuration context
    ///
    /// # Arguments
    /// * `fs_name` - Filesystem type (e.g., "tmpfs")
    /// * `flags` - Flags (e.g., FSOPEN_CLOEXEC)
    pub fn open(fs_name: &CStr, flags: u32) -> Result<Self, std::io::Error> {
        unsafe {
            let fd = fsopen(fs_name, flags);
            if fd < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(FsContext {
                fd: OwnedFd::from_raw_fd(fd),
            })
        }
    }

    /// Set a string configuration parameter
    ///
    /// # Arguments
    /// * `key` - Configuration key
    /// * `value` - String value
    pub fn set_string(&self, key: &CStr, value: &CStr) -> Result<(), std::io::Error> {
        unsafe {
            let ret = fsconfig(
                self.fd.as_raw_fd(),
                FSCONFIG_SET_STRING,
                key.as_ptr(),
                value.as_ptr() as *const libc::c_void,
                0,
            );
            if ret < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        }
    }

    /// Set a flag configuration parameter
    ///
    /// # Arguments
    /// * `key` - Flag name
    pub fn set_flag(&self, key: &CStr) -> Result<(), std::io::Error> {
        unsafe {
            let ret = fsconfig(
                self.fd.as_raw_fd(),
                FSCONFIG_SET_FLAG,
                key.as_ptr(),
                std::ptr::null(),
                0,
            );
            if ret < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        }
    }

    /// Finalize the filesystem configuration
    ///
    /// This triggers filesystem creation
    pub fn create(&self) -> Result<(), std::io::Error> {
        unsafe {
            let ret = fsconfig(
                self.fd.as_raw_fd(),
                FSCONFIG_CMD_CREATE,
                std::ptr::null(),
                std::ptr::null(),
                0,
            );
            if ret < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        }
    }

    /// Create a detached mount from this configuration
    ///
    /// # Arguments
    /// * `attr_flags` - Mount attribute flags (usually 0)
    /// * `flags` - Mount flags (e.g., FSMOUNT_CLOEXEC)
    pub fn mount(self, attr_flags: u32, flags: u32) -> Result<MountFd, std::io::Error> {
        unsafe {
            let mount_fd = fsmount(self.fd.as_raw_fd(), flags, attr_flags);
            if mount_fd < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(MountFd {
                fd: OwnedFd::from_raw_fd(mount_fd),
            })
        }
    }
}

impl AsRawFd for FsContext {
    fn as_raw_fd(&self) -> RawFd {
        self.fd.as_raw_fd()
    }
}

/// RAII wrapper for mount file descriptor
///
/// Represents a detached mount created by fsmount() or open_tree().
/// The fd is automatically closed when this struct is dropped.
pub struct MountFd {
    fd: OwnedFd,
}

impl MountFd {
    /// Create a detached copy of a mount tree
    ///
    /// # Arguments
    /// * `dfd` - Directory file descriptor (or use a RawFd like AT_FDCWD)
    /// * `path` - Path to the mount point
    /// * `flags` - Flags (typically OPEN_TREE_CLONE | AT_RECURSIVE)
    pub fn open_tree<Fd: AsFd>(dfd: Fd, path: &CStr, flags: u32) -> Result<Self, std::io::Error> {
        unsafe {
            let mount_fd = open_tree(dfd.as_fd().as_raw_fd(), path.as_ptr(), flags);
            if mount_fd < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(MountFd {
                fd: OwnedFd::from_raw_fd(mount_fd),
            })
        }
    }

    /// Create a detached copy of a mount tree using AT_FDCWD
    ///
    /// # Arguments
    /// * `path` - Path to the mount point
    /// * `flags` - Flags (typically OPEN_TREE_CLONE | AT_RECURSIVE)
    pub fn open_tree_at(path: &CStr, flags: u32) -> Result<Self, std::io::Error> {
        unsafe {
            let mount_fd = open_tree(AT_FDCWD, path.as_ptr(), flags);
            if mount_fd < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(MountFd {
                fd: OwnedFd::from_raw_fd(mount_fd),
            })
        }
    }

    /// Move this mount to a target location
    ///
    /// # Arguments
    /// * `to_dfd` - Destination directory fd (or AT_FDCWD)
    /// * `to_path` - Destination path
    /// * `flags` - Movement flags
    pub fn attach_to(
        self,
        to_dfd: RawFd,
        to_path: &CStr,
        flags: u32,
    ) -> Result<(), std::io::Error> {
        unsafe {
            let empty_path = c"";
            let ret = move_mount(
                self.fd.as_raw_fd(),
                empty_path.as_ptr(),
                to_dfd,
                to_path.as_ptr(),
                flags | MOVE_MOUNT_F_EMPTY_PATH,
            );
            if ret < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        }
    }

    /// Create from a raw fd, taking ownership
    ///
    /// # Safety
    /// The fd must be valid and the caller must ensure it's not used elsewhere
    pub unsafe fn from_raw_fd(fd: RawFd) -> Self {
        unsafe {
            MountFd {
                fd: OwnedFd::from_raw_fd(fd),
            }
        }
    }
}

impl AsRawFd for MountFd {
    fn as_raw_fd(&self) -> RawFd {
        self.fd.as_raw_fd()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syscalls::capability;
    use crate::test_utils::run_in_userns;
    use std::ffi::CString;

    /// Helper to check if we can run mount API tests
    fn require_mount_api() -> bool {
        if !capability::has_mount_api() {
            eprintln!("Skipping test: mount API not available on this kernel");
            return false;
        }
        true
    }

    #[test]
    fn test_fscontext_create_tmpfs() {
        if !require_mount_api() {
            return;
        }

        run_in_userns(|| {
            let fs_name = CString::new("tmpfs").unwrap();
            let fs_ctx = match FsContext::open(&fs_name, FSOPEN_CLOEXEC) {
                Ok(ctx) => ctx,
                Err(e) => {
                    eprintln!("fsopen failed: {}", e);
                    unsafe { libc::_exit(1) };
                }
            };

            if let Err(e) = fs_ctx.create() {
                eprintln!("create failed: {}", e);
                unsafe { libc::_exit(1) };
            }

            let _mount_fd = match fs_ctx.mount(0, FSMOUNT_CLOEXEC) {
                Ok(mnt) => mnt,
                Err(e) => {
                    eprintln!("fsmount failed: {}", e);
                    unsafe { libc::_exit(1) };
                }
            };

            unsafe { libc::_exit(0) };
        });
    }

    #[test]
    fn test_fscontext_with_config() {
        if !require_mount_api() {
            return;
        }

        run_in_userns(|| {
            let fs_name = CString::new("tmpfs").unwrap();
            let fs_ctx = match FsContext::open(&fs_name, FSOPEN_CLOEXEC) {
                Ok(ctx) => ctx,
                Err(e) => {
                    eprintln!("fsopen failed: {}", e);
                    unsafe { libc::_exit(1) };
                }
            };

            let source_key = CString::new("source").unwrap();
            let source_value = CString::new("cntr_test").unwrap();
            if let Err(e) = fs_ctx.set_string(&source_key, &source_value) {
                eprintln!("set_string failed: {}", e);
                unsafe { libc::_exit(1) };
            }

            if let Err(e) = fs_ctx.create() {
                eprintln!("create failed: {}", e);
                unsafe { libc::_exit(1) };
            }

            let _mount_fd = match fs_ctx.mount(0, FSMOUNT_CLOEXEC) {
                Ok(mnt) => mnt,
                Err(e) => {
                    eprintln!("fsmount failed: {}", e);
                    unsafe { libc::_exit(1) };
                }
            };

            unsafe { libc::_exit(0) };
        });
    }

    #[test]
    fn test_mountfd_attach() {
        if !require_mount_api() {
            return;
        }

        run_in_userns(|| {
            let fs_name = CString::new("tmpfs").unwrap();
            let fs_ctx = match FsContext::open(&fs_name, FSOPEN_CLOEXEC) {
                Ok(ctx) => ctx,
                Err(e) => {
                    eprintln!("fsopen failed: {}", e);
                    unsafe { libc::_exit(1) };
                }
            };

            if let Err(e) = fs_ctx.create() {
                eprintln!("create failed: {}", e);
                unsafe { libc::_exit(1) };
            }

            let mount_fd = match fs_ctx.mount(0, FSMOUNT_CLOEXEC) {
                Ok(mnt) => mnt,
                Err(e) => {
                    eprintln!("fsmount failed: {}", e);
                    unsafe { libc::_exit(1) };
                }
            };

            let mount_point = CString::new("/tmp/cntr_test_attach").unwrap();
            let _ = std::fs::create_dir("/tmp/cntr_test_attach");

            if let Err(e) = mount_fd.attach_to(AT_FDCWD, &mount_point, 0) {
                eprintln!("attach_to failed: {}", e);
                let _ = std::fs::remove_dir("/tmp/cntr_test_attach");
                unsafe { libc::_exit(1) };
            }

            unsafe {
                libc::umount2(mount_point.as_ptr(), libc::MNT_DETACH);
            }
            let _ = std::fs::remove_dir("/tmp/cntr_test_attach");

            unsafe { libc::_exit(0) };
        });
    }
}
