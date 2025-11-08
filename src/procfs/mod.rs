use libc::c_ulong;
use nix::unistd::Pid;
use simple_error::{SimpleError, try_with};
use std::env;
use std::ffi::OsString;
use std::fs::File;
use std::io::BufReader;
use std::io::prelude::*;
use std::path::PathBuf;

use crate::result::Result;

mod unix;

pub fn get_path() -> PathBuf {
    PathBuf::from(&env::var_os("CNTR_PROC").unwrap_or_else(|| OsString::from("/proc")))
}

#[derive(Clone)]
pub struct ProcStatus {
    pub global_pid: Pid,
    pub effective_capabilities: c_ulong,
}

pub fn status(target_pid: Pid) -> Result<ProcStatus> {
    let path = get_path().join(target_pid.to_string()).join("status");
    let file = try_with!(File::open(&path), "failed to open {}", path.display());

    let mut effective_caps: Option<c_ulong> = None;

    let reader = BufReader::new(file);
    for line in reader.lines() {
        let line = try_with!(line, "could not read {}", path.display());
        let columns: Vec<&str> = line.split('\t').collect();
        assert!(columns.len() >= 2);
        if columns[0] == "CapEff:"
            && let Some(cap_string) = columns.last()
        {
            let cap = try_with!(
                c_ulong::from_str_radix(cap_string, 16),
                "read invalid capability from proc: '{}'",
                columns[1]
            );
            effective_caps = Some(cap);
        }
    }

    Ok(ProcStatus {
        global_pid: target_pid,
        effective_capabilities: effective_caps.ok_or_else(|| {
            SimpleError::new(format!(
                "Could not find effective capabilities in {}",
                path.display()
            ))
        })?,
    })
}
