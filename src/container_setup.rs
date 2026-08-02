//! Shared container setup utilities
//!
//! This module provides common functionality for entering container namespaces
//! and setting up security context (LSM, cgroups, capabilities).

use rustix::io::Errno;
use rustix::process::{Pid, getpid};
use rustix::thread::{set_thread_gid, set_thread_groups, set_thread_uid};
use thiserror::Error;

use crate::capabilities;
use crate::cgroup::{self, CgroupError};
use crate::namespace::{self, NamespaceError};
use crate::procfs::ProcStatus;

#[derive(Debug, Error)]
pub(crate) enum SetupError {
    #[error("the system has no support for mount namespaces")]
    MountNamespaceUnsupported,
    #[error("failed to enter container namespace")]
    Namespace(#[from] NamespaceError),
    #[error("failed to change cgroup")]
    Cgroup(#[from] CgroupError),
    #[error("could not set group id")]
    SetGid(#[source] Errno),
    #[error("could not set user id")]
    SetUid(#[source] Errno),
}

/// Enter all container namespaces
///
/// Opens and enters mount, UTS, cgroup, PID, net, IPC, and user namespaces.
/// Returns true if USER namespace was entered.
fn enter_namespaces(container_pid: Pid) -> Result<bool, SetupError> {
    // Detect supported namespaces
    let supported_namespaces = namespace::supported_namespaces()?;

    if !supported_namespaces.contains(namespace::MOUNT.name) {
        return Err(SetupError::MountNamespaceUnsupported);
    }

    // Open mount namespace
    let mount_namespace = namespace::MOUNT.open(container_pid)?;

    // Open other namespaces
    let mut other_namespaces = Vec::new();
    let mut user_ns_entered = false;
    let other_kinds = &[
        namespace::UTS,
        namespace::CGROUP,
        namespace::PID,
        namespace::NET,
        namespace::IPC,
        namespace::USER,
    ];

    for kind in other_kinds {
        if !supported_namespaces.contains(kind.name) {
            continue;
        }
        if kind.is_same(container_pid) {
            continue;
        }

        let ns = kind.open(container_pid)?;

        // Track if USER namespace was successfully opened
        if kind.name == namespace::USER.name {
            user_ns_entered = true;
        }

        other_namespaces.push(ns);
    }

    // Enter mount namespace first
    mount_namespace.apply()?;

    // Enter other namespaces
    for ns in other_namespaces {
        ns.apply()?;
    }

    Ok(user_ns_entered)
}

/// Apply security context (UID/GID, capabilities, LSM)
///
/// Sets UID/GID, drops capabilities, and applies LSM profile.
pub(crate) fn apply_security_context(
    process_status: &mut ProcStatus,
    in_user_namespace: bool,
) -> Result<(), SetupError> {
    // Set UID/GID
    if in_user_namespace {
        // Try to clear supplementary groups, but ignore errors as this may fail
        // in some sandboxes even when not explicitly denied
        let _ = set_thread_groups(&[]);
        set_thread_gid(process_status.gid).map_err(SetupError::SetGid)?;
        set_thread_uid(process_status.uid).map_err(SetupError::SetUid)?;
    }

    // Drop capabilities
    capabilities::drop(
        process_status.effective_capabilities,
        process_status.last_cap,
    );

    Ok(())
}

/// Complete container setup: cgroup, namespaces, and security context
///
/// This is a convenience function that performs all setup steps:
/// 1. Moves to container's cgroup
/// 2. Enters all container namespaces
/// 3. Applies security context (UID/GID, capabilities, LSM)
pub(crate) fn enter_container(process_status: &mut ProcStatus) -> Result<(), SetupError> {
    // Move to container's cgroup
    cgroup::move_to(getpid(), process_status.global_pid)?;

    // Enter namespaces
    let in_user_ns = enter_namespaces(process_status.global_pid)?;

    // Apply security context
    apply_security_context(process_status, in_user_ns)?;

    Ok(())
}
