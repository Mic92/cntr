//! Shared container access utilities
//!
//! This module provides common functionality for looking up containers
//! and accessing their properties.

use crate::procfs;
use crate::result::Result;
use nix::unistd::Pid;
use simple_error::{bail, try_with};
use std::fs::metadata;
use std::os::unix::fs::MetadataExt;

/// Container context with all necessary information for attaching/execing
pub(crate) struct ContainerContext {
    pub(crate) uid: nix::unistd::Uid,
    pub(crate) gid: nix::unistd::Gid,
    pub(crate) process_status: procfs::ProcStatus,
}

impl ContainerContext {
    /// Lookup a container by name/ID and get its context
    ///
    /// # Arguments
    /// * `container_name` - Container name, ID, or PID
    /// * `container_types` - List of container types to try
    pub(crate) fn lookup(
        container_name: &str,
        container_types: &[Box<dyn container_pid::Container>],
    ) -> Result<Self> {
        // Lookup container PID
        let pid_raw = match container_pid::lookup_container_pid(container_name, container_types) {
            Ok(pid) => pid,
            Err(e) => bail!("{}", e),
        };
        let pid = Pid::from_raw(pid_raw);

        // Get container uid/gid from process metadata
        let metadata = try_with!(
            metadata(procfs::get_path().join(pid.to_string())),
            "failed to get container uid/gid"
        );
        let uid = nix::unistd::Uid::from_raw(metadata.uid());
        let gid = nix::unistd::Gid::from_raw(metadata.gid());

        // Get process status
        let process_status = try_with!(procfs::status(pid), "failed to get process status");

        Ok(ContainerContext {
            uid,
            gid,
            process_status,
        })
    }
}
