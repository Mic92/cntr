use anyhow::Context;
use libc::c_ulong;
use nix::unistd::{Gid, Pid, Uid};
use std::env;
use std::ffi::OsString;
use std::fs::File;
use std::io::BufReader;
use std::io::prelude::*;
use std::path::{Path, PathBuf};

use crate::ApparmorMode;
use crate::lsm::LSMProfile;
use crate::result::Result;

pub(crate) fn get_path() -> PathBuf {
    PathBuf::from(&env::var_os("CNTR_PROC").unwrap_or_else(|| OsString::from("/proc")))
}

/// Parse a uid_map or gid_map file and translate an outer ID to inner ID
///
/// Format: `id-inside id-outside length`
/// Example: `0 100000 65536` means container ID 0 maps to host ID 100000
fn translate_id(map_path: &Path, outer_id: u32) -> Result<u32> {
    let contents = std::fs::read_to_string(map_path)
        .with_context(|| format!("failed to read {:?}", map_path))?;

    for line in contents.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() != 3 {
            continue;
        }

        let inner_start: u32 = parts[0]
            .parse()
            .with_context(|| format!("failed to parse inner ID in {:?}", map_path))?;
        let outer_start: u32 = parts[1]
            .parse()
            .with_context(|| format!("failed to parse outer ID in {:?}", map_path))?;
        let length: u32 = parts[2]
            .parse()
            .with_context(|| format!("failed to parse length in {:?}", map_path))?;

        // Check if outer_id falls within this mapping range
        // Use checked arithmetic to avoid overflow
        if let Some(offset) = outer_id.checked_sub(outer_start)
            && offset < length
        {
            let inner = inner_start.checked_add(offset).ok_or_else(|| {
                anyhow::anyhow!(
                    "integer overflow computing inner ID in {:?}: {} + {} would overflow",
                    map_path,
                    inner_start,
                    offset
                )
            })?;
            return Ok(inner);
        }
    }

    // No mapping found - ID is unmapped, use as-is
    // This happens when the process is not in a user namespace
    Ok(outer_id)
}

pub(crate) struct ProcStatus {
    pub(crate) global_pid: Pid,
    pub(crate) effective_capabilities: c_ulong,
    pub(crate) last_cap: c_ulong,
    pub(crate) uid: Uid,
    pub(crate) gid: Gid,
    pub(crate) lsm_profile: Option<LSMProfile>,
}

pub(crate) fn status(target_pid: Pid, apparmor_mode: ApparmorMode) -> Result<ProcStatus> {
    let path = get_path().join(target_pid.to_string()).join("status");
    let file = File::open(&path)
        .with_context(|| format!("failed to open process status file {}", path.display()))?;

    let mut effective_caps: Option<c_ulong> = None;

    let reader = BufReader::new(file);
    for line in reader.lines() {
        let line = line.with_context(|| format!("could not read line from {}", path.display()))?;
        let columns: Vec<&str> = line.split('\t').collect();
        if columns.len() < 2 {
            anyhow::bail!(
                "malformed line in {} (expected at least 2 tab-separated columns, found {}): '{}'",
                path.display(),
                columns.len(),
                line
            );
        }
        if columns[0] == "CapEff:"
            && let Some(cap_string) = columns.last()
        {
            let cap = c_ulong::from_str_radix(cap_string, 16).with_context(|| {
                format!(
                    "failed to parse capability '{}' from {}",
                    cap_string,
                    path.display()
                )
            })?;
            effective_caps = Some(cap);
        }
    }

    let effective_capabilities = effective_caps.ok_or_else(|| {
        anyhow::anyhow!(
            "could not find effective capabilities (CapEff) in {}",
            path.display()
        )
    })?;

    // Read cap_last_cap from the host namespace before entering the target namespace
    let cap_last_cap_path = get_path().join("sys/kernel/cap_last_cap");
    let cap_contents = std::fs::read_to_string(&cap_last_cap_path)
        .with_context(|| format!("failed to read {}", cap_last_cap_path.display()))?;
    let cap_contents_trimmed = cap_contents.trim();
    let last_cap = cap_contents_trimmed.parse::<c_ulong>().with_context(|| {
        format!(
            "failed to parse last capability value from {}: '{}'",
            cap_last_cap_path.display(),
            cap_contents_trimmed
        )
    })?;

    // Get container uid/gid from process metadata (host perspective)
    use std::fs::metadata;
    use std::os::unix::fs::MetadataExt;

    let proc_dir = get_path().join(target_pid.to_string());
    let metadata = metadata(&proc_dir)
        .with_context(|| format!("failed to get metadata for {}", proc_dir.display()))?;
    let host_uid = metadata.uid();
    let host_gid = metadata.gid();

    // Translate host UID/GID to container namespace UID/GID
    let container_uid = translate_id(&proc_dir.join("uid_map"), host_uid)
        .with_context(|| format!("failed to translate host UID {} to container UID", host_uid))?;
    let container_gid = translate_id(&proc_dir.join("gid_map"), host_gid)
        .with_context(|| format!("failed to translate host GID {} to container GID", host_gid))?;

    let uid = Uid::from_raw(container_uid);
    let gid = Gid::from_raw(container_gid);

    // Read LSM profile
    let lsm_profile =
        crate::lsm::read_profile(target_pid, apparmor_mode).context("failed to get lsm profile")?;

    Ok(ProcStatus {
        global_pid: target_pid,
        effective_capabilities,
        last_cap,
        uid,
        gid,
        lsm_profile,
    })
}
