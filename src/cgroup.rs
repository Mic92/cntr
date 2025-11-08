use anyhow::Context;
use log::warn;
use nix::unistd;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;

use crate::procfs;
use crate::result::Result;

fn get_subsystems() -> Result<Vec<String>> {
    let path = "/proc/cgroups";
    let f = File::open(path).context("failed to open /proc/cgroups")?;
    let reader = BufReader::new(f);
    let mut subsystems: Vec<String> = Vec::new();
    for l in reader.lines() {
        let line = l.context("failed to read line from /proc/cgroups")?;
        if line.starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = line.split('\t').collect();
        if fields.len() >= 4 && fields[3] != "0" {
            subsystems.push(fields[0].to_string());
        }
    }
    Ok(subsystems)
}

fn get_mounts() -> Result<HashMap<String, String>> {
    let subsystems =
        get_subsystems().context("failed to obtain cgroup subsystems from /proc/cgroups")?;
    let path = "/proc/self/mountinfo";
    // example:
    //
    // 36 35 98:0 /mnt1 /mnt2 rw,noatime master:1 - ext3 /dev/root rw,errors=continue
    // (1)(2)(3)   (4)   (5)      (6)      (7)   (8) (9)   (10)         (11)
    let f = File::open(path).context("failed to open /proc/self/mountinfo")?;
    let reader = BufReader::new(f);
    let mut mountpoints: HashMap<String, String> = HashMap::new();
    for l in reader.lines() {
        let line = l.with_context(|| format!("failed to read line from {}", path))?;
        let fields: Vec<&str> = line.split(' ').collect();
        if fields.len() < 11 || fields[9] != "cgroup" {
            continue;
        }
        for option in fields[10].split(',') {
            let name = option.strip_prefix("name=").unwrap_or(option).to_string();
            if !subsystems.contains(&name) {
                mountpoints.insert(name, fields[4].to_string());
            }
        }
    }
    Ok(mountpoints)
}

fn get_cgroups(pid: unistd::Pid) -> Result<Vec<String>> {
    let path = procfs::get_path().join(format!("{}/cgroup", pid));
    let f = File::open(&path)
        .with_context(|| format!("failed to open cgroup file {}", path.display()))?;
    let reader = BufReader::new(f);
    let mut cgroups: Vec<String> = Vec::new();
    for l in reader.lines() {
        let line = l.with_context(|| format!("failed to read line from {}", path.display()))?;
        let fields: Vec<&str> = line.split(":/").collect();
        if fields.len() >= 2 {
            cgroups.push(fields[1].to_string());
        }
    }
    Ok(cgroups)
}

fn cgroup_path(cgroup: &str, mountpoints: &HashMap<String, String>) -> Option<PathBuf> {
    for c in cgroup.split(',') {
        let m = mountpoints.get(c);
        if let Some(path) = m {
            let mut tasks_path = PathBuf::from(path);
            tasks_path.push(cgroup);
            tasks_path.push("tasks");
            return Some(tasks_path);
        }
    }
    None
}

// TODO add implementation for unified cgroups, cgmanager, lxcfs
// -> on the long run everything will be done with unified cgroups hopefully

pub(crate) fn move_to(pid: unistd::Pid, target_pid: unistd::Pid) -> Result<()> {
    let cgroups = get_cgroups(target_pid)
        .with_context(|| format!("failed to get cgroups for PID {}", target_pid))?;
    let mountpoints = get_mounts().context("failed to get cgroup mountpoints")?;
    for cgroup in cgroups {
        let p = cgroup_path(&cgroup, &mountpoints);
        if let Some(path) = p {
            match File::create(&path) {
                Ok(mut buffer) => {
                    write!(buffer, "{}", pid)
                        .with_context(|| format!("failed to write PID to cgroup {}", cgroup))?;
                }
                Err(err) => {
                    warn!("failed to enter {} namespace: {}", cgroup, err);
                }
            }
        }
    }
    Ok(())
}
