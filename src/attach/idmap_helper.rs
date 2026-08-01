use crate::syscalls::process::{Fork, exit, fork};
use log::debug;
use rustix::io::read;
use rustix::pipe::pipe;
use rustix::process::{Gid, Pid, Signal, Uid, WaitOptions, kill_process, waitpid};
use rustix::thread::{UnshareFlags, unshare_unsafe};
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use thiserror::Error;

use crate::fsutil;

use rustix::io::Errno;

#[derive(Debug, Error)]
pub(crate) enum IdmapError {
    #[error("failed to create sync pipe")]
    CreatePipe(#[source] Errno),
    #[error("failed to fork idmap helper")]
    Fork(#[source] Errno),
    #[error("failed to read from helper")]
    ReadFromHelper(#[source] Errno),
    #[error("helper failed during setup (read {0} bytes, expected 1)")]
    HelperNotReady(usize),
    #[error("failed to open {path}")]
    OpenUserns {
        path: String,
        #[source]
        source: Errno,
    },
    #[error("failed to unshare user namespace")]
    UnshareUserns(#[source] Errno),
    #[error("failed to write {path}")]
    WriteIdMap {
        path: &'static str,
        #[source]
        source: Errno,
    },
}

/// Helper process that creates and maintains a user namespace for idmapped mounts
pub(super) struct IdmapHelper {
    _pid: Pid,
    userns_fd: OwnedFd,
}

impl IdmapHelper {
    /// Create a user namespace with specific UID/GID mapping
    ///
    /// Maps: inner_uid (inside userns) -> outer_uid (outside userns)
    ///
    /// For idmapped mounts: files created by inner_uid appear as owned by outer_uid on host.
    /// Typically: inner_uid=current_uid (e.g., root), outer_uid=target_uid (e.g., joerg)
    pub(super) fn new(
        inner_uid: Uid,
        outer_uid: Uid,
        inner_gid: Gid,
        outer_gid: Gid,
    ) -> Result<Self, IdmapError> {
        // Create sync pipe
        let (read_fd, write_fd) = pipe().map_err(IdmapError::CreatePipe)?;

        match fork().map_err(IdmapError::Fork)? {
            Fork::ParentOf(child) => {
                // Close write end
                drop(write_fd);

                // Wait for child to be ready
                let mut buf = [0u8; 1];
                let bytes_read = read(&read_fd, &mut buf).map_err(IdmapError::ReadFromHelper)?;
                if bytes_read != 1 {
                    return Err(IdmapError::HelperNotReady(bytes_read));
                }
                drop(read_fd);

                // Open child's user namespace
                let userns_path = format!("/proc/{}/ns/user", child);
                let userns_fd =
                    fsutil::open_read(&userns_path).map_err(|source| IdmapError::OpenUserns {
                        path: userns_path.clone(),
                        source,
                    })?;

                debug!(
                    "Created idmap helper (PID {}) mapping {}:{} -> {}:{}",
                    child,
                    inner_uid.as_raw(),
                    inner_gid.as_raw(),
                    outer_uid.as_raw(),
                    outer_gid.as_raw()
                );

                Ok(IdmapHelper {
                    _pid: child,
                    userns_fd,
                })
            }
            Fork::Child => {
                // Close read end
                drop(read_fd);

                // Create user namespace and set up mapping
                if let Err(e) = Self::setup_userns(inner_uid, outer_uid, inner_gid, outer_gid) {
                    eprintln!("idmap helper failed: {}", crate::errors::format_chain(&e));
                    exit(1);
                }

                // Signal parent we're ready
                if let Err(e) = rustix::io::write(&write_fd, b"R") {
                    eprintln!("idmap helper failed to signal parent: {:?}", e);
                    exit(1);
                }

                // Keep running (parent holds FD, but this is safer)
                loop {
                    let _ = rustix::thread::nanosleep(&rustix::thread::Timespec {
                        tv_sec: 3600,
                        tv_nsec: 0,
                    });
                }
            }
        }
    }

    fn setup_userns(
        inner_uid: Uid,
        outer_uid: Uid,
        inner_gid: Gid,
        outer_gid: Gid,
    ) -> Result<(), IdmapError> {
        // Create user namespace
        unsafe { unshare_unsafe(UnshareFlags::NEWUSER) }.map_err(IdmapError::UnshareUserns)?;

        // Disable setgroups
        fsutil::write("/proc/self/setgroups", b"deny").ok();

        // Write uid_map: inner_uid (inside userns) -> outer_uid (outside userns)
        let uid_map = format!("{} {} 1\n", inner_uid.as_raw(), outer_uid.as_raw());
        fsutil::write("/proc/self/uid_map", uid_map).map_err(|source| IdmapError::WriteIdMap {
            path: "/proc/self/uid_map",
            source,
        })?;

        // Write gid_map: inner_gid (inside userns) -> outer_gid (outside userns)
        let gid_map = format!("{} {} 1\n", inner_gid.as_raw(), outer_gid.as_raw());
        fsutil::write("/proc/self/gid_map", gid_map).map_err(|source| IdmapError::WriteIdMap {
            path: "/proc/self/gid_map",
            source,
        })?;

        Ok(())
    }

    /// Get the user namespace FD
    pub(super) fn userns_fd(&self) -> BorrowedFd<'_> {
        self.userns_fd.as_fd()
    }
}

impl Drop for IdmapHelper {
    fn drop(&mut self) {
        // Kill helper and reap it
        kill_process(self._pid, Signal::KILL).ok();
        waitpid(Some(self._pid), WaitOptions::empty()).ok();
    }
}
