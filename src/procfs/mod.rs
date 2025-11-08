use anyhow::Context;
use libc::c_ulong;
use nix::unistd::Pid;
use std::env;
use std::ffi::OsString;
use std::fs::File;
use std::io::BufReader;
use std::io::prelude::*;
use std::path::PathBuf;

use crate::result::Result;

pub(crate) fn get_path() -> PathBuf {
    PathBuf::from(&env::var_os("CNTR_PROC").unwrap_or_else(|| OsString::from("/proc")))
}

#[derive(Clone)]
pub(crate) struct ProcStatus {
    pub(crate) global_pid: Pid,
    pub(crate) effective_capabilities: c_ulong,
    pub(crate) last_cap: c_ulong,
}

pub(crate) fn status(target_pid: Pid) -> Result<ProcStatus> {
    let path = get_path().join(target_pid.to_string()).join("status");
    let file = File::open(&path)
        .with_context(|| format!("failed to open process status file {}", path.display()))?;

    let mut effective_caps: Option<c_ulong> = None;

    let reader = BufReader::new(file);
    for line in reader.lines() {
        let line = line.with_context(|| format!("could not read line from {}", path.display()))?;
        let columns: Vec<&str> = line.split('\t').collect();
        assert!(columns.len() >= 2);
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

    Ok(ProcStatus {
        global_pid: target_pid,
        effective_capabilities,
        last_cap,
    })
}
