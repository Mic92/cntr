use anyhow::{Context, Result, bail};
use log::debug;
use rustix::io::read;
use rustix::pipe::pipe;
use rustix::process::{Gid, Pid, Signal, Uid, WaitOptions, kill_process, waitpid};
use rustix::runtime::{Fork, exit_group, kernel_fork};
use rustix::thread::{UnshareFlags, unshare_unsafe};
use std::fs::File;
use std::io::Write;
use std::os::fd::{AsFd, BorrowedFd};

/// Helper process that creates and maintains a user namespace for idmapped mounts
pub(super) struct IdmapHelper {
    _pid: Pid,
    userns_fd: File,
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
    ) -> Result<Self> {
        // Create sync pipe
        let (read_fd, write_fd) = pipe().context("failed to create sync pipe")?;

        // SAFETY: cntr is single-threaded and uses a rustix-based global allocator,
        // so allocating and calling into std in the child is fine.
        match unsafe { kernel_fork() }.context("failed to fork idmap helper")? {
            Fork::ParentOf(child) => {
                // Close write end
                drop(write_fd);

                // Wait for child to be ready
                let mut buf = [0u8; 1];
                let bytes_read = read(&read_fd, &mut buf).context("failed to read from helper")?;
                if bytes_read != 1 {
                    bail!(
                        "helper failed during setup (read {} bytes, expected 1)",
                        bytes_read
                    );
                }
                drop(read_fd);

                // Open child's user namespace
                let userns_path = format!("/proc/{}/ns/user", child);
                let userns_fd = File::open(&userns_path)
                    .with_context(|| format!("failed to open {}", userns_path))?;

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
            Fork::Child(_) => {
                // Close read end
                drop(read_fd);

                // Create user namespace and set up mapping
                if let Err(e) = Self::setup_userns(inner_uid, outer_uid, inner_gid, outer_gid) {
                    eprintln!("idmap helper failed: {:?}", e);
                    exit_group(1);
                }

                // Signal parent we're ready
                let mut write_file = File::from(write_fd);
                if let Err(e) = write_file.write_all(b"R") {
                    eprintln!("idmap helper failed to signal parent: {:?}", e);
                    exit_group(1);
                }

                // Keep running (parent holds FD, but this is safer)
                loop {
                    std::thread::sleep(std::time::Duration::from_secs(3600));
                }
            }
        }
    }

    fn setup_userns(inner_uid: Uid, outer_uid: Uid, inner_gid: Gid, outer_gid: Gid) -> Result<()> {
        // Create user namespace
        unsafe { unshare_unsafe(UnshareFlags::NEWUSER) }
            .context("failed to unshare user namespace")?;

        // Disable setgroups
        std::fs::write("/proc/self/setgroups", b"deny").ok();

        // Write uid_map: inner_uid (inside userns) -> outer_uid (outside userns)
        let uid_map = format!("{} {} 1\n", inner_uid.as_raw(), outer_uid.as_raw());
        std::fs::write("/proc/self/uid_map", uid_map.as_bytes())
            .context("failed to write uid_map")?;

        // Write gid_map: inner_gid (inside userns) -> outer_gid (outside userns)
        let gid_map = format!("{} {} 1\n", inner_gid.as_raw(), outer_gid.as_raw());
        std::fs::write("/proc/self/gid_map", gid_map.as_bytes())
            .context("failed to write gid_map")?;

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
