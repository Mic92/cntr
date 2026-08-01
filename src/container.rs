//! Shared container access utilities
//!
//! This module provides common functionality for looking up containers
//! and accessing their properties.

use crate::ApparmorMode;
use crate::procfs::{self, ProcfsError};
use rustix::process::Pid;
use thiserror::Error;

#[derive(Debug, Error)]
pub(crate) enum ContainerError {
    #[error("{0}")]
    Lookup(String),
    #[error("invalid PID {pid} for container '{container}'")]
    InvalidPid { pid: i32, container: String },
    #[error("failed to read process status of container")]
    Procfs(#[from] ProcfsError),
}

/// Lookup a container by name/ID and get its process status
///
/// # Arguments
/// * `container_name` - Container name, ID, or PID
/// * `container_types` - List of container types to try
/// * `apparmor_mode` - AppArmor mode configuration
pub(crate) fn lookup_container(
    container_name: &str,
    container_types: &[Box<dyn container_pid::Container>],
    apparmor_mode: ApparmorMode,
) -> Result<procfs::ProcStatus, ContainerError> {
    // Lookup container PID
    let pid_raw = container_pid::lookup_container_pid(container_name, container_types)
        .map_err(|e| ContainerError::Lookup(e.to_string()))?;
    let pid = Pid::from_raw(pid_raw).ok_or_else(|| ContainerError::InvalidPid {
        pid: pid_raw,
        container: container_name.to_string(),
    })?;

    // Get process status (includes uid, gid, capabilities, lsm_profile)
    Ok(procfs::status(pid, apparmor_mode)?)
}
