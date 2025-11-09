//! Shared container setup utilities
//!
//! This module provides common functionality for entering container namespaces
//! and setting up security context (LSM, cgroups, capabilities).

use anyhow::{Context, bail};
use nix::unistd::{self, Pid};

use crate::capabilities;
use crate::cgroup;
use crate::namespace;
use crate::procfs::ProcStatus;
use crate::result::Result;

/// Enter all container namespaces
///
/// Opens and enters mount, UTS, cgroup, PID, net, IPC, and user namespaces.
/// Returns true if USER namespace was entered.
fn enter_namespaces(container_pid: Pid) -> Result<bool> {
    // Detect supported namespaces
    let supported_namespaces =
        namespace::supported_namespaces().context("failed to list namespaces")?;

    if !supported_namespaces.contains(namespace::MOUNT.name) {
        bail!("the system has no support for mount namespaces");
    }

    // Open mount namespace
    let mount_namespace = namespace::MOUNT
        .open(container_pid)
        .context("could not access mount namespace")?;

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

        let ns = kind
            .open(container_pid)
            .with_context(|| format!("failed to open {} namespace", kind.name))?;

        // Track if USER namespace was successfully opened
        if kind.name == namespace::USER.name {
            user_ns_entered = true;
        }

        other_namespaces.push(ns);
    }

    // Enter mount namespace first
    mount_namespace
        .apply()
        .context("failed to enter mount namespace")?;

    // Enter other namespaces
    for ns in other_namespaces {
        ns.apply().context("failed to apply namespace")?;
    }

    Ok(user_ns_entered)
}

/// Apply security context (UID/GID, capabilities, LSM)
///
/// Sets UID/GID, drops capabilities, and applies LSM profile.
pub(crate) fn apply_security_context(
    process_status: &mut ProcStatus,
    in_user_namespace: bool,
) -> Result<()> {
    // Set UID/GID
    if in_user_namespace {
        // Try to clear supplementary groups, but ignore errors as this may fail
        // in some sandboxes even when not explicitly denied
        let _ = unistd::setgroups(&[]);
        unistd::setgid(process_status.gid).context("could not set group id")?;
        unistd::setuid(process_status.uid).context("could not set user id")?;
    }

    // Drop capabilities
    capabilities::drop(
        process_status.effective_capabilities,
        process_status.last_cap,
    )
    .context("failed to apply capabilities")?;

    // Inherit LSM profile
    if let Some(profile) = &mut process_status.lsm_profile {
        profile
            .inherit_profile()
            .context("failed to inherit lsm profile")?;
    }

    Ok(())
}

/// Complete container setup: cgroup, namespaces, and security context
///
/// This is a convenience function that performs all setup steps:
/// 1. Moves to container's cgroup
/// 2. Enters all container namespaces
/// 3. Applies security context (UID/GID, capabilities, LSM)
pub(crate) fn enter_container(process_status: &mut ProcStatus) -> Result<()> {
    // Move to container's cgroup
    cgroup::move_to(unistd::getpid(), process_status.global_pid)
        .context("failed to change cgroup")?;

    // Enter namespaces
    let in_user_ns = enter_namespaces(process_status.global_pid).with_context(|| {
        format!(
            "failed to enter namespaces for PID {}",
            process_status.global_pid
        )
    })?;

    // Apply security context
    apply_security_context(process_status, in_user_ns).with_context(|| {
        format!(
            "failed to apply security context (UID={}, GID={})",
            process_status.uid, process_status.gid
        )
    })?;

    Ok(())
}
