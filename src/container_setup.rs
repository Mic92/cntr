//! Shared container setup utilities
//!
//! This module provides common functionality for entering container namespaces
//! and setting up security context (LSM, cgroups, capabilities).

use anyhow::{Context, bail};
use nix::unistd::{self, Gid, Pid, Uid};
use simple_error::{bail, try_with};

use crate::capabilities;
use crate::cgroup;
use crate::lsm::{self, LSMProfile};
use crate::namespace;
use crate::procfs::ProcStatus;
use crate::result::Result;

/// Container security context
pub(crate) struct SecurityContext {
    pub(crate) lsm_profile: Option<LSMProfile>,
    pub(crate) uid: Uid,
    pub(crate) gid: Gid,
}

/// Prepare security context for container entry
///
/// This reads the LSM profile and container UID/GID before entering namespaces.
pub(crate) fn prepare_security_context(
    container_pid: Pid,
    _process_status: &ProcStatus,
) -> Result<SecurityContext> {
    // Read LSM profile before entering namespaces
    let lsm_profile = try_with!(
        lsm::read_profile(container_pid),
        "failed to get lsm profile"
    );

    // Get container uid/gid from process metadata
    use crate::procfs;
    use std::fs::metadata;
    use std::os::unix::fs::MetadataExt;

    let metadata = try_with!(
        metadata(procfs::get_path().join(container_pid.to_string())),
        "failed to get container uid/gid"
    );
    let uid = Uid::from_raw(metadata.uid());
    let gid = Gid::from_raw(metadata.gid());

    Ok(SecurityContext {
        lsm_profile,
        uid,
        gid,
    })
}

/// Enter all container namespaces
///
/// Opens and enters mount, UTS, cgroup, PID, net, IPC, and user namespaces.
/// Returns true if USER namespace was entered.
pub(crate) fn enter_namespaces(container_pid: Pid) -> Result<bool> {
    // Detect supported namespaces
    let supported_namespaces = try_with!(
        namespace::supported_namespaces(),
        "failed to list namespaces"
    );

    if !supported_namespaces.contains(namespace::MOUNT.name) {
        bail!("the system has no support for mount namespaces");
    }

    // Open mount namespace
    let mount_namespace = try_with!(
        namespace::MOUNT.open(container_pid),
        "could not access mount namespace"
    );

    // Open other namespaces
    let mut other_namespaces = Vec::new();
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

        other_namespaces.push(try_with!(
            kind.open(container_pid),
            "failed to open {} namespace",
            kind.name
        ));
    }

    // Enter mount namespace first
    try_with!(mount_namespace.apply(), "failed to enter mount namespace");

    // Enter other namespaces
    for ns in other_namespaces {
        try_with!(ns.apply(), "failed to apply namespace");
    }

    Ok(supported_namespaces.contains(namespace::USER.name))
}

/// Apply security context (UID/GID, capabilities, LSM)
///
/// Sets UID/GID, drops capabilities, and applies LSM profile.
pub(crate) fn apply_security_context(
    ctx: SecurityContext,
    process_status: &ProcStatus,
    in_user_namespace: bool,
) -> Result<()> {
    // Set UID/GID
    if in_user_namespace {
        // Check if setgroups is already denied
        let setgroups_denied = std::fs::read_to_string("/proc/self/setgroups")
            .map(|s| s.trim() == "deny")
            .unwrap_or(false);

        let dropped_groups = if !setgroups_denied {
            unistd::setgroups(&[]).is_ok()
        } else {
            setgroups_denied
        };

        if !setgroups_denied
            && let Err(e) = unistd::setgroups(&[])
            && !dropped_groups
        {
            try_with!(Err(e), "could not set groups");
        }
        try_with!(unistd::setgid(ctx.gid), "could not set group id");
        try_with!(unistd::setuid(ctx.uid), "could not set user id");
    }

    // Drop capabilities
    try_with!(
        capabilities::drop(process_status.effective_capabilities),
        "failed to apply capabilities"
    );

    // Inherit LSM profile
    if let Some(profile) = ctx.lsm_profile {
        try_with!(profile.inherit_profile(), "failed to inherit lsm profile");
    }

    Ok(())
}

/// Complete container setup: cgroup, namespaces, and security context
///
/// This is a convenience function that performs all setup steps:
/// 1. Moves to container's cgroup
/// 2. Prepares security context (reads LSM, UID/GID)
/// 3. Enters all container namespaces
/// 4. Applies security context (UID/GID, capabilities, LSM)
pub(crate) fn enter_container(container_pid: Pid, process_status: &ProcStatus) -> Result<()> {
    // Move to container's cgroup
    try_with!(
        cgroup::move_to(unistd::getpid(), container_pid),
        "failed to change cgroup"
    );

    // Prepare security context
    let ctx = prepare_security_context(container_pid, process_status)?;

    // Enter namespaces
    let in_user_ns = enter_namespaces(container_pid)?;

    // Apply security context
    apply_security_context(ctx, process_status, in_user_ns)?;

    Ok(())
}
