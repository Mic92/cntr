use anyhow::Context;
use log::{debug, warn};
use nix::unistd;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;

use crate::procfs;
use crate::result::Result;

/// Trait for cgroup operations, supporting both v1 and v2
trait CgroupManager {
    /// Move a process into the cgroup of another process
    fn move_to(&self, pid: unistd::Pid, target_pid: unistd::Pid) -> Result<()>;
}

/// Cgroup v1 (legacy) manager
struct CgroupV1Manager {
    procfs_path: PathBuf,
}

/// Cgroup v2 (unified) manager
struct CgroupV2Manager {
    mount_path: PathBuf,
    procfs_path: PathBuf,
}

/// Hybrid manager that supports both v1 and v2
struct HybridCgroupManager {
    v1: CgroupV1Manager,
    v2: CgroupV2Manager,
}

/// Null manager for systems without cgroup support
struct NullCgroupManager;

// Helper functions for cgroup v1

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
            // Fixed: only insert if name IS a valid subsystem
            if subsystems.contains(&name) {
                mountpoints.insert(name, fields[4].to_string());
            }
        }
    }
    Ok(mountpoints)
}

fn cgroup_v1_path(cgroup: &str, mountpoints: &HashMap<String, String>) -> Option<PathBuf> {
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

// Cgroup v1 implementation
impl CgroupV1Manager {
    fn get_cgroups(&self, pid: unistd::Pid) -> Result<Vec<String>> {
        let path = self.procfs_path.join(format!("{}/cgroup", pid));
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
}

impl CgroupManager for CgroupV1Manager {
    fn move_to(&self, pid: unistd::Pid, target_pid: unistd::Pid) -> Result<()> {
        let cgroups = self
            .get_cgroups(target_pid)
            .with_context(|| format!("failed to get cgroups for PID {}", target_pid))?;
        let mountpoints = get_mounts().context("failed to get cgroup mountpoints")?;

        for cgroup in cgroups {
            let p = cgroup_v1_path(&cgroup, &mountpoints);
            if let Some(path) = p {
                match File::create(&path) {
                    Ok(mut buffer) => {
                        write!(buffer, "{}", pid)
                            .with_context(|| format!("failed to write PID to cgroup {}", cgroup))?;
                    }
                    Err(err) => {
                        warn!("failed to enter {} cgroup: {}", cgroup, err);
                    }
                }
            }
        }
        Ok(())
    }
}

// Cgroup v2 implementation
impl CgroupV2Manager {
    fn get_cgroup_path(&self, pid: unistd::Pid) -> Result<Option<String>> {
        let path = self.procfs_path.join(format!("{}/cgroup", pid));
        let f = File::open(&path)
            .with_context(|| format!("failed to open cgroup file {}", path.display()))?;
        let reader = BufReader::new(f);

        for l in reader.lines() {
            let line = l.with_context(|| format!("failed to read line from {}", path.display()))?;
            // cgroup v2 format: "0::/path/to/cgroup"
            if let Some(stripped) = line.strip_prefix("0::") {
                return Ok(Some(stripped.to_string()));
            }
        }
        Ok(None)
    }
}

impl CgroupManager for CgroupV2Manager {
    fn move_to(&self, pid: unistd::Pid, target_pid: unistd::Pid) -> Result<()> {
        let target_cgroup = self
            .get_cgroup_path(target_pid)
            .with_context(|| format!("failed to get cgroup v2 path for PID {}", target_pid))?;

        let Some(cgroup_path) = target_cgroup else {
            warn!(
                "PID {} not in a cgroup v2, skipping cgroup migration",
                target_pid
            );
            return Ok(());
        };

        // Build path: /sys/fs/cgroup/<cgroup_path>/cgroup.procs
        let mut procs_path = self.mount_path.clone();
        procs_path.push(cgroup_path.trim_start_matches('/'));
        procs_path.push("cgroup.procs");

        match File::options().append(true).open(&procs_path) {
            Ok(mut file) => {
                write!(file, "{}", pid).with_context(|| {
                    format!(
                        "failed to write PID to cgroup.procs at {}",
                        procs_path.display()
                    )
                })?;
            }
            Err(err) => {
                warn!(
                    "failed to open cgroup.procs at {}: {}",
                    procs_path.display(),
                    err
                );
            }
        }

        Ok(())
    }
}

// Hybrid implementation - tries v2 first, falls back to v1
impl CgroupManager for HybridCgroupManager {
    fn move_to(&self, pid: unistd::Pid, target_pid: unistd::Pid) -> Result<()> {
        // Try v2 first
        if let Err(e) = self.v2.move_to(pid, target_pid) {
            warn!("cgroup v2 migration failed: {}, trying v1", e);
            self.v1.move_to(pid, target_pid)?;
        }
        Ok(())
    }
}

// Null implementation - no-op when cgroups are unavailable
impl CgroupManager for NullCgroupManager {
    fn move_to(&self, _pid: unistd::Pid, _target_pid: unistd::Pid) -> Result<()> {
        debug!("cgroup support not detected, skipping cgroup migration");
        Ok(())
    }
}

/// Factory function to create the appropriate CgroupManager
fn create_manager() -> Result<Box<dyn CgroupManager>> {
    let path = "/proc/self/mountinfo";
    let f = File::open(path).context("failed to open /proc/self/mountinfo")?;
    let reader = BufReader::new(f);

    let mut has_v1 = false;
    let mut v2_mount: Option<PathBuf> = None;

    for l in reader.lines() {
        let line = l.with_context(|| format!("failed to read line from {}", path))?;
        let fields: Vec<&str> = line.split(' ').collect();
        if fields.len() < 10 {
            continue;
        }
        if fields[9] == "cgroup" {
            has_v1 = true;
        } else if fields[9] == "cgroup2" {
            v2_mount = Some(PathBuf::from(fields[4]));
        }
    }

    let procfs_path = procfs::get_path();

    match (has_v1, v2_mount) {
        (true, Some(mount_path)) => {
            // Hybrid: both v1 and v2
            Ok(Box::new(HybridCgroupManager {
                v1: CgroupV1Manager {
                    procfs_path: procfs_path.clone(),
                },
                v2: CgroupV2Manager {
                    mount_path,
                    procfs_path,
                },
            }))
        }
        (true, None) => {
            // Only v1
            Ok(Box::new(CgroupV1Manager { procfs_path }))
        }
        (false, Some(mount_path)) => {
            // Only v2
            Ok(Box::new(CgroupV2Manager {
                mount_path,
                procfs_path,
            }))
        }
        (false, None) => {
            // No cgroups found, use null manager
            Ok(Box::new(NullCgroupManager))
        }
    }
}

/// Move a process into the cgroup of another process
pub(crate) fn move_to(pid: unistd::Pid, target_pid: unistd::Pid) -> Result<()> {
    let manager = create_manager().context("failed to create cgroup manager")?;
    manager.move_to(pid, target_pid)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write as IoWrite;

    #[test]
    fn test_cgroup_v1_path_construction() {
        let mut mountpoints = HashMap::new();
        mountpoints.insert("cpu".to_string(), "/sys/fs/cgroup/cpu".to_string());
        mountpoints.insert("memory".to_string(), "/sys/fs/cgroup/memory".to_string());

        // Test single controller
        let result = cgroup_v1_path("cpu", &mountpoints);
        assert_eq!(result, Some(PathBuf::from("/sys/fs/cgroup/cpu/cpu/tasks")));

        // Test non-existent controller
        let result = cgroup_v1_path("blkio", &mountpoints);
        assert_eq!(result, None);
    }

    #[test]
    fn test_cgroup_v2_path_parses_correctly() {
        // Create a temporary proc directory structure
        let temp_dir = std::env::temp_dir().join(format!("cntr_test_{}", std::process::id()));
        let pid_dir = temp_dir.join("12345");
        fs::create_dir_all(&pid_dir).unwrap();

        // Write a test cgroup file with v2 format
        let cgroup_file = pid_dir.join("cgroup");
        let mut file = fs::File::create(&cgroup_file).unwrap();
        writeln!(file, "0::/user.slice/user-1000.slice/session-3.scope").unwrap();

        // Create manager with mock procfs path
        let manager = CgroupV2Manager {
            mount_path: PathBuf::from("/sys/fs/cgroup"),
            procfs_path: temp_dir.clone(),
        };

        let result = manager
            .get_cgroup_path(unistd::Pid::from_raw(12345))
            .unwrap();

        // Clean up
        fs::remove_dir_all(&temp_dir).unwrap();

        assert_eq!(
            result,
            Some("/user.slice/user-1000.slice/session-3.scope".to_string())
        );
    }

    #[test]
    fn test_cgroup_v2_path_returns_none_for_v1() {
        // Create a temporary proc directory structure
        let temp_dir = std::env::temp_dir().join(format!("cntr_test_v1_{}", std::process::id()));
        let pid_dir = temp_dir.join("12346");
        fs::create_dir_all(&pid_dir).unwrap();

        // Write a test cgroup file with v1 format (no "0::" prefix)
        let cgroup_file = pid_dir.join("cgroup");
        let mut file = fs::File::create(&cgroup_file).unwrap();
        writeln!(file, "1:name=systemd:/user.slice").unwrap();
        writeln!(file, "2:cpu,cpuacct:/user.slice").unwrap();

        let manager = CgroupV2Manager {
            mount_path: PathBuf::from("/sys/fs/cgroup"),
            procfs_path: temp_dir.clone(),
        };

        let result = manager
            .get_cgroup_path(unistd::Pid::from_raw(12346))
            .unwrap();

        fs::remove_dir_all(&temp_dir).unwrap();

        assert_eq!(result, None);
    }
}
