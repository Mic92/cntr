use hashbrown::HashMap;
use log::{debug, warn};
use rustix::io::Errno;
use rustix::process::Pid;
use thiserror::Error;
use typed_path::UnixPathBuf;

use crate::errors::format_chain;
use crate::fsutil;
use crate::procfs;

#[derive(Debug, Error)]
pub(crate) enum CgroupError {
    #[error("failed to read {}", path.display())]
    Read {
        path: UnixPathBuf,
        #[source]
        source: Errno,
    },
}

/// Trait for cgroup operations, supporting both v1 and v2
trait CgroupManager {
    /// Move a process into the cgroup of another process
    fn move_to(&self, pid: Pid, target_pid: Pid) -> Result<(), CgroupError>;
}

/// Cgroup v1 (legacy) manager
struct CgroupV1Manager {
    procfs_path: UnixPathBuf,
}

/// Cgroup v2 (unified) manager
struct CgroupV2Manager {
    mount_path: UnixPathBuf,
    procfs_path: UnixPathBuf,
}

/// Hybrid manager that supports both v1 and v2
struct HybridCgroupManager {
    v1: CgroupV1Manager,
    v2: CgroupV2Manager,
}

/// Null manager for systems without cgroup support
struct NullCgroupManager;

// Helper functions for cgroup v1

fn get_subsystems() -> Result<Vec<String>, CgroupError> {
    let path = "/proc/cgroups";
    let contents = fsutil::read_to_string(path).map_err(|source| CgroupError::Read {
        path: UnixPathBuf::from(path),
        source,
    })?;
    let mut subsystems: Vec<String> = Vec::new();
    for line in contents.lines() {
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

fn get_mounts() -> Result<HashMap<String, String>, CgroupError> {
    let subsystems = get_subsystems()?;
    let path = "/proc/self/mountinfo";
    // example:
    //
    // 36 35 98:0 /mnt1 /mnt2 rw,noatime master:1 - ext3 /dev/root rw,errors=continue
    // (1)(2)(3)   (4)   (5)      (6)      (7)   (8) (9)   (10)         (11)
    let contents = fsutil::read_to_string(path).map_err(|source| CgroupError::Read {
        path: UnixPathBuf::from(path),
        source,
    })?;
    let mut mountpoints: HashMap<String, String> = HashMap::new();
    for line in contents.lines() {
        let fields: Vec<&str> = line.split(' ').collect();
        if fields.len() < 11 || fields[9] != "cgroup" {
            continue;
        }
        for option in fields[10].split(',') {
            let name = option.strip_prefix("name=").unwrap_or(option).to_string();
            // Only mounts of real subsystems are useful for cgroup_v1_path()
            if subsystems.contains(&name) {
                mountpoints.insert(name, fields[4].to_string());
            }
        }
    }
    Ok(mountpoints)
}

fn cgroup_v1_path(cgroup: &str, mountpoints: &HashMap<String, String>) -> Option<UnixPathBuf> {
    for c in cgroup.split(',') {
        let m = mountpoints.get(c);
        if let Some(path) = m {
            let mut tasks_path = UnixPathBuf::from(path);
            tasks_path.push(cgroup);
            tasks_path.push("tasks");
            return Some(tasks_path);
        }
    }
    None
}

// Cgroup v1 implementation
impl CgroupV1Manager {
    fn get_cgroups(&self, pid: Pid) -> Result<Vec<String>, CgroupError> {
        let path = self.procfs_path.join(format!("{}/cgroup", pid));
        let contents = fsutil::read_to_string(&path).map_err(|source| CgroupError::Read {
            path: path.clone(),
            source,
        })?;
        let mut cgroups: Vec<String> = Vec::new();
        for line in contents.lines() {
            let fields: Vec<&str> = line.split(":/").collect();
            if fields.len() >= 2 {
                cgroups.push(fields[1].to_string());
            }
        }
        Ok(cgroups)
    }
}

impl CgroupManager for CgroupV1Manager {
    fn move_to(&self, pid: Pid, target_pid: Pid) -> Result<(), CgroupError> {
        let cgroups = self.get_cgroups(target_pid)?;
        let mountpoints = get_mounts()?;

        for cgroup in cgroups {
            let p = cgroup_v1_path(&cgroup, &mountpoints);
            if let Some(path) = p {
                if let Err(err) = fsutil::write(&path, pid.to_string()) {
                    warn!("failed to enter {} cgroup: {}", cgroup, err);
                }
            }
        }
        Ok(())
    }
}

// Cgroup v2 implementation
impl CgroupV2Manager {
    fn get_cgroup_path(&self, pid: Pid) -> Result<Option<String>, CgroupError> {
        let path = self.procfs_path.join(format!("{}/cgroup", pid));
        let contents = fsutil::read_to_string(&path).map_err(|source| CgroupError::Read {
            path: path.clone(),
            source,
        })?;

        for line in contents.lines() {
            // cgroup v2 format: "0::/path/to/cgroup"
            if let Some(stripped) = line.strip_prefix("0::") {
                return Ok(Some(stripped.to_string()));
            }
        }
        Ok(None)
    }
}

impl CgroupManager for CgroupV2Manager {
    fn move_to(&self, pid: Pid, target_pid: Pid) -> Result<(), CgroupError> {
        let target_cgroup = self.get_cgroup_path(target_pid)?;

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

        if let Err(err) = fsutil::append(&procs_path, pid.to_string()) {
            // Writing to cgroup.procs requires CAP_SYS_ADMIN or root.
            // For unprivileged users (e.g., rootless podman), warn and continue.
            warn!(
                "failed to write PID to cgroup.procs at {}: {} (try running as root or with CAP_SYS_ADMIN)",
                procs_path.display(),
                err
            );
        }

        Ok(())
    }
}

// Hybrid implementation - tries v2 first, falls back to v1
impl CgroupManager for HybridCgroupManager {
    fn move_to(&self, pid: Pid, target_pid: Pid) -> Result<(), CgroupError> {
        // Try v2 first
        if let Err(e) = self.v2.move_to(pid, target_pid) {
            warn!(
                "cgroup v2 migration failed: {}, trying v1",
                format_chain(&e)
            );
            self.v1.move_to(pid, target_pid)?;
        }
        Ok(())
    }
}

// Null implementation - no-op when cgroups are unavailable
impl CgroupManager for NullCgroupManager {
    fn move_to(&self, _pid: Pid, _target_pid: Pid) -> Result<(), CgroupError> {
        debug!("cgroup support not detected, skipping cgroup migration");
        Ok(())
    }
}

/// Factory function to create the appropriate CgroupManager
fn create_manager() -> Result<Box<dyn CgroupManager>, CgroupError> {
    let path = "/proc/self/mountinfo";
    let contents = fsutil::read_to_string(path).map_err(|source| CgroupError::Read {
        path: UnixPathBuf::from(path),
        source,
    })?;

    let mut has_v1 = false;
    let mut v2_mount: Option<UnixPathBuf> = None;

    for line in contents.lines() {
        let fields: Vec<&str> = line.split(' ').collect();
        if fields.len() < 10 {
            continue;
        }
        if fields[9] == "cgroup" {
            has_v1 = true;
        } else if fields[9] == "cgroup2" {
            v2_mount = Some(UnixPathBuf::from(fields[4]));
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
pub(crate) fn move_to(pid: Pid, target_pid: Pid) -> Result<(), CgroupError> {
    let manager = create_manager()?;
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
        assert_eq!(
            result,
            Some(UnixPathBuf::from("/sys/fs/cgroup/cpu/cpu/tasks"))
        );

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
            mount_path: UnixPathBuf::from("/sys/fs/cgroup"),
            procfs_path: UnixPathBuf::from(temp_dir.to_str().unwrap()),
        };

        let result = manager
            .get_cgroup_path(Pid::from_raw(12345).unwrap())
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
            mount_path: UnixPathBuf::from("/sys/fs/cgroup"),
            procfs_path: UnixPathBuf::from(temp_dir.to_str().unwrap()),
        };

        let result = manager
            .get_cgroup_path(Pid::from_raw(12346).unwrap())
            .unwrap();

        fs::remove_dir_all(&temp_dir).unwrap();

        assert_eq!(result, None);
    }
}
