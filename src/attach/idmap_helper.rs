use anyhow::{Context, Result};
use log::debug;
use nix::sys::wait::waitpid;
use nix::unistd::{ForkResult, Gid, Pid, Uid, fork};
use std::fs::File;
use std::os::unix::io::AsRawFd;

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
        let (read_fd, write_fd) = nix::unistd::pipe().context("failed to create sync pipe")?;

        match unsafe { fork() }.context("failed to fork idmap helper")? {
            ForkResult::Parent { child } => {
                // Close write end
                drop(write_fd);

                // Wait for child to be ready
                let mut buf = [0u8; 1];
                let bytes_read =
                    nix::unistd::read(&read_fd, &mut buf).context("failed to read from helper")?;
                if bytes_read != 1 {
                    anyhow::bail!(
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
                    child, inner_uid, inner_gid, outer_uid, outer_gid
                );

                Ok(IdmapHelper {
                    _pid: child,
                    userns_fd,
                })
            }
            ForkResult::Child => {
                // Close read end
                drop(read_fd);

                // Create user namespace and set up mapping
                if let Err(e) = Self::setup_userns(inner_uid, outer_uid, inner_gid, outer_gid) {
                    eprintln!("idmap helper failed: {}", e);
                    unsafe { libc::_exit(1) };
                }

                // Signal parent we're ready
                nix::unistd::write(&write_fd, b"R").ok();
                drop(write_fd);

                // Keep running (parent holds FD, but this is safer)
                loop {
                    std::thread::sleep(std::time::Duration::from_secs(3600));
                }
            }
        }
    }

    fn setup_userns(inner_uid: Uid, outer_uid: Uid, inner_gid: Gid, outer_gid: Gid) -> Result<()> {
        use nix::sched::{CloneFlags, unshare};

        // Create user namespace
        unshare(CloneFlags::CLONE_NEWUSER).context("failed to unshare user namespace")?;

        // Disable setgroups
        std::fs::write("/proc/self/setgroups", b"deny").ok();

        // Write uid_map: inner_uid (inside userns) -> outer_uid (outside userns)
        let uid_map = format!("{} {} 1\n", inner_uid, outer_uid);
        std::fs::write("/proc/self/uid_map", uid_map.as_bytes())
            .context("failed to write uid_map")?;

        // Write gid_map: inner_gid (inside userns) -> outer_gid (outside userns)
        let gid_map = format!("{} {} 1\n", inner_gid, outer_gid);
        std::fs::write("/proc/self/gid_map", gid_map.as_bytes())
            .context("failed to write gid_map")?;

        Ok(())
    }

    /// Get the user namespace FD
    pub(super) fn userns_fd(&self) -> std::os::unix::io::BorrowedFd<'_> {
        unsafe { std::os::unix::io::BorrowedFd::borrow_raw(self.userns_fd.as_raw_fd()) }
    }
}

impl Drop for IdmapHelper {
    fn drop(&mut self) {
        // Kill helper and reap it
        use nix::sys::signal::{Signal, kill};
        kill(self._pid, Signal::SIGKILL).ok();
        waitpid(self._pid, None).ok();
    }
}
