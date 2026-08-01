use rustix::process::{Gid, Pid, Uid};
use std::env;
use std::ffi::OsString;
use std::fs::File;
use std::io::BufReader;
use std::io::prelude::*;
use std::path::{Path, PathBuf};
use thiserror::Error;

use crate::ApparmorMode;
use crate::lsm::{LSMProfile, LsmError};

#[derive(Debug, Error)]
pub(crate) enum ProcfsError {
    #[error("failed to open {path}")]
    Open {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to read {path}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse {what} in {path}: '{value}'")]
    Parse {
        what: &'static str,
        path: PathBuf,
        value: String,
        #[source]
        source: std::num::ParseIntError,
    },
    #[error(
        "integer overflow computing inner ID in {path}: {inner_start} + {offset} would overflow"
    )]
    IdOverflow {
        path: PathBuf,
        inner_start: u32,
        offset: u32,
    },
    #[error(
        "malformed line in {path} (expected at least 2 tab-separated columns, found {columns}): '{line}'"
    )]
    MalformedStatusLine {
        path: PathBuf,
        columns: usize,
        line: String,
    },
    #[error("could not find effective capabilities (CapEff) in {path}")]
    MissingCapEff { path: PathBuf },
    #[error("failed to get metadata for {path}")]
    Metadata {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to read LSM profile")]
    Lsm(#[from] LsmError),
}

pub(crate) fn get_path() -> PathBuf {
    PathBuf::from(&env::var_os("CNTR_PROC").unwrap_or_else(|| OsString::from("/proc")))
}

/// Parse a uid_map or gid_map file and translate an outer ID to inner ID
///
/// Format: `id-inside id-outside length`
/// Example: `0 100000 65536` means container ID 0 maps to host ID 100000
fn translate_id(map_path: &Path, outer_id: u32) -> Result<u32, ProcfsError> {
    let contents = std::fs::read_to_string(map_path).map_err(|source| ProcfsError::Read {
        path: map_path.to_path_buf(),
        source,
    })?;

    let parse = |what: &'static str, value: &str| {
        value.parse::<u32>().map_err(|source| ProcfsError::Parse {
            what,
            path: map_path.to_path_buf(),
            value: value.to_string(),
            source,
        })
    };

    for line in contents.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() != 3 {
            continue;
        }

        let inner_start = parse("inner ID", parts[0])?;
        let outer_start = parse("outer ID", parts[1])?;
        let length = parse("length", parts[2])?;

        // Check if outer_id falls within this mapping range
        // Use checked arithmetic to avoid overflow
        if let Some(offset) = outer_id.checked_sub(outer_start) {
            if offset < length {
                let inner =
                    inner_start
                        .checked_add(offset)
                        .ok_or_else(|| ProcfsError::IdOverflow {
                            path: map_path.to_path_buf(),
                            inner_start,
                            offset,
                        })?;
                return Ok(inner);
            }
        }
    }

    // No mapping found - ID is unmapped, use as-is
    // This happens when the process is not in a user namespace
    Ok(outer_id)
}

pub(crate) struct ProcStatus {
    pub(crate) global_pid: Pid,
    pub(crate) effective_capabilities: u64,
    pub(crate) last_cap: u64,
    pub(crate) uid: Uid,
    pub(crate) gid: Gid,
    pub(crate) lsm_profile: Option<LSMProfile>,
}

pub(crate) fn status(
    target_pid: Pid,
    apparmor_mode: ApparmorMode,
) -> Result<ProcStatus, ProcfsError> {
    let path = get_path().join(target_pid.to_string()).join("status");
    let file = File::open(&path).map_err(|source| ProcfsError::Open {
        path: path.clone(),
        source,
    })?;

    let mut effective_caps: Option<u64> = None;

    let reader = BufReader::new(file);
    for line in reader.lines() {
        let line = line.map_err(|source| ProcfsError::Read {
            path: path.clone(),
            source,
        })?;
        let columns: Vec<&str> = line.split('\t').collect();
        if columns.len() < 2 {
            return Err(ProcfsError::MalformedStatusLine {
                path,
                columns: columns.len(),
                line,
            });
        }
        if columns[0] == "CapEff:" {
            if let Some(cap_string) = columns.last() {
                let cap =
                    u64::from_str_radix(cap_string, 16).map_err(|source| ProcfsError::Parse {
                        what: "capability",
                        path: path.clone(),
                        value: cap_string.to_string(),
                        source,
                    })?;
                effective_caps = Some(cap);
            }
        }
    }

    let effective_capabilities =
        effective_caps.ok_or_else(|| ProcfsError::MissingCapEff { path: path.clone() })?;

    // Read cap_last_cap from the host namespace before entering the target namespace
    let cap_last_cap_path = get_path().join("sys/kernel/cap_last_cap");
    let cap_contents =
        std::fs::read_to_string(&cap_last_cap_path).map_err(|source| ProcfsError::Read {
            path: cap_last_cap_path.clone(),
            source,
        })?;
    let cap_contents_trimmed = cap_contents.trim();
    let last_cap = cap_contents_trimmed
        .parse::<u64>()
        .map_err(|source| ProcfsError::Parse {
            what: "last capability value",
            path: cap_last_cap_path.clone(),
            value: cap_contents_trimmed.to_string(),
            source,
        })?;

    // Get container uid/gid from process metadata (host perspective)
    use std::fs::metadata;
    use std::os::unix::fs::MetadataExt;

    let proc_dir = get_path().join(target_pid.to_string());
    let metadata = metadata(&proc_dir).map_err(|source| ProcfsError::Metadata {
        path: proc_dir.clone(),
        source,
    })?;
    let host_uid = metadata.uid();
    let host_gid = metadata.gid();

    // Translate host UID/GID to container namespace UID/GID
    let container_uid = translate_id(&proc_dir.join("uid_map"), host_uid)?;
    let container_gid = translate_id(&proc_dir.join("gid_map"), host_gid)?;

    let uid = Uid::from_raw(container_uid);
    let gid = Gid::from_raw(container_gid);

    // Read LSM profile
    let lsm_profile = crate::lsm::read_profile(target_pid, apparmor_mode)?;

    Ok(ProcStatus {
        global_pid: target_pid,
        effective_capabilities,
        last_cap,
        uid,
        gid,
        lsm_profile,
    })
}
