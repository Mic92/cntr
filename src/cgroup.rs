use cntr_nix::unistd;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::io::Write;
use std::path::PathBuf;
use types::{Error, Result};

fn get_subsystems() -> Result<Vec<String>> {
    let path = "/proc/cgroups";
    let f = tryfmt!(File::open(&path), "failed to open /proc/cgroups");
    let reader = BufReader::new(f);
    let mut subsystems: Vec<String> = Vec::new();
    for l in reader.lines() {
        let line = tryfmt!(l, "failed to read /proc/cgroups");
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
    let subsystems = tryfmt!(get_subsystems(), "failed to obtain cgroup subsystems");
    let path = "/proc/self/mountinfo";
    // example:
    //
    // 36 35 98:0 /mnt1 /mnt2 rw,noatime master:1 - ext3 /dev/root rw,errors=continue
    // (1)(2)(3)   (4)   (5)      (6)      (7)   (8) (9)   (10)         (11)
    let f = tryfmt!(File::open(&path), "failed to read /proc/self/mountinfo");
    let reader = BufReader::new(f);
    let mut mountpoints: HashMap<String, String> = HashMap::new();
    for l in reader.lines() {
        let line = tryfmt!(l, "failed to read '{}'", path);
        let fields: Vec<&str> = line.split(' ').collect();
        if fields.len() < 11 || fields[9] != "cgroup" {
            continue;
        }
        for option in fields[10].split(',') {
            let name = if option.starts_with("name=") {
                option[5..].to_string()
            } else {
                option.to_string()
            };
            if !subsystems.contains(&name) {
                mountpoints.insert(name, fields[4].to_string());
            }
        }
    }
    Ok(mountpoints)
}

fn get_cgroups(pid: unistd::Pid) -> Result<Vec<String>> {
    let path = format!("/proc/{}/cgroup", pid);
    let f = tryfmt!(File::open(&path), "failed to read {}", path);
    let reader = BufReader::new(f);
    let mut cgroups: Vec<String> = Vec::new();
    for l in reader.lines() {
        let line = tryfmt!(l, "failed to read '{}'", path);
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
        if m.is_some() {
            let mut tasks_path = PathBuf::from(m.unwrap());
            tasks_path.push(cgroup);
            tasks_path.push("tasks");
            return Some(tasks_path);
        }
    }
    None
}

// TODO add implementation for unified cgroups, cgmanager, lxcfs
// -> on the long run everything will be done with unified cgroups hopefully

pub fn move_to(pid: unistd::Pid, target_pid: unistd::Pid) -> Result<()> {
    let cgroups = tryfmt!(
        get_cgroups(target_pid),
        "failed to get cgroups of {}",
        target_pid
    );
    let mountpoints = tryfmt!(get_mounts(), "failed to get cgroup mountpoints");
    for cgroup in cgroups {
        let p = cgroup_path(&cgroup, &mountpoints);
        if p.is_some() {
            let path = p.unwrap();
            match File::create(&path) {
                Ok(mut buffer) => {
                    tryfmt!(
                        write!(buffer, "{}", pid),
                        "failed to enter {} cgroup",
                        cgroup
                    );
                }
                Err(err) => {
                    warn!("failed to enter {} namespace: {}", cgroup, err);
                }
            }
        }
    }
    Ok(())
}
