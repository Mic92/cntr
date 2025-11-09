//! Shared container access utilities
//!
//! This module provides common functionality for looking up containers
//! and accessing their properties.

use crate::procfs;
use crate::result::Result;
use anyhow::bail;
use nix::unistd::Pid;

/// Lookup a container by name/ID and get its process status
///
/// # Arguments
/// * `container_name` - Container name, ID, or PID
/// * `container_types` - List of container types to try
pub(crate) fn lookup_container(
    container_name: &str,
    container_types: &[Box<dyn container_pid::Container>],
) -> Result<procfs::ProcStatus> {
    // Lookup container PID
    let pid_raw = match container_pid::lookup_container_pid(container_name, container_types) {
        Ok(pid) => pid,
        Err(e) => bail!("{}", e),
    };
    let pid = Pid::from_raw(pid_raw);

    // Get process status (includes uid, gid, capabilities, lsm_profile)
    procfs::status(pid)
}
