//! This module uses `kubectl` to get a containerd id and then searches cgroups for one with that
//! id as name. It returns any pid which is a member of that group.
//!
//! Possible container_id inputs:
//!
//! - `podname` to use default namespace and first container in that pod
//! - one `/`: `namespace/podname` to override default namespace
//! - two `/`: `namespace/podname/container` to be super explicit

use crate::container_pid::Container;
use crate::container_pid::Error;
use crate::container_pid::RawPid;
use crate::container_pid::cmd;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::str::FromStr;
use std::str::from_utf8;

#[derive(Clone, Debug)]
pub(crate) struct Kubernetes {}

pub(crate) const DEFAULT_NAMESPACE: &str = "default";

impl Container for Kubernetes {
    /// There is many ways to do this:
    ///  - similar to command.rs: a bit looser pattern matching on /proc/$pid/cmdline
    ///  - the following:
    fn lookup(&self, container_id: &str) -> Result<RawPid, Error> {
        let (namespace, pod_name, container_name) = parse_userinput(container_id);
        let containerdid = get_containerd_id(namespace, pod_name, container_name)?;
        let cgroup = find_cgroup(containerdid)?;
        let pid = get_cgroup_pid(&cgroup)?;
        Ok(pid)
    }

    fn check_required_tools(&self) -> Result<(), Error> {
        if cmd::which("kubectl").is_some() {
            Ok(())
        } else {
            Err(Error::RuntimeNotFound {
                runtime: "kubernetes",
                tool: "kubectl",
            })
        }
    }
}

/// allows the user to prepend the pod name with `custom-namespace/pod-name` to override the
/// namespace (`default`). By default this will take the first container of the pod. That however
/// can be overridden by appending it like `namespace/podname/container`.
pub(crate) fn parse_userinput(container_id: &str) -> (&str, &str, Option<&str>) {
    let fields = container_id.splitn(3, '/').collect::<Vec<&str>>();
    match fields.as_slice() {
        [pod_name] => (DEFAULT_NAMESPACE, pod_name, None),
        [namespace, pod_name] => (namespace, pod_name, None),
        [namespace, pod_name, container] => (namespace, pod_name, Some(container)),
        _ => unreachable!(),
    }
}

/// find `containerd://hash` id and return hash.
/// Potentially vulnerable: passes unchecked user supplied strings to command.
pub(crate) fn get_containerd_id(
    namespace: &str,
    pod_name: &str,
    container_name: Option<&str>,
) -> Result<String, Error> {
    let jsonpath = format!(
        "jsonpath='{{range .items[?(@.metadata.name==\"{}\")].status.containerStatuses[*]}}{{.name}}{{\"\\t\"}}{{.containerID}}{{\"\\n\"}}{{end}}'",
        pod_name
    );
    let result = Command::new("kubectl")
        .arg("get")
        .arg("pod")
        .arg("-o")
        .arg(jsonpath)
        .arg("-n")
        .arg(namespace)
        .output()
        .map_err(|source| Error::CommandFailedToRun {
            command: "kubectl get pod".to_string(),
            source,
        })?;

    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        return Err(Error::CommandFailed {
            command: "kubectl get pod".to_string(),
            status: result.status.to_string(),
            stderr: stderr.to_string(),
        });
    }

    let containers = from_utf8(&result.stdout).map_err(|_| Error::UnexpectedOutput {
        command: "kubectl get pod".to_string(),
        message: "response contains non-UTF8 data".to_string(),
    })?;
    let containerid = containers.split('\n').find_map(|line| {
        // line = "containername\tcontainerdid"
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() != 2 {
            return None;
        }
        if let Some(name) = container_name {
            // return name-matching containerid
            if cols[0] == name {
                return Some(cols[1]);
            }
        } else {
            // return any containerid
            return Some(cols[1]);
        }
        None
    });

    let containerid = containerid.ok_or_else(|| Error::ContainerNotFound {
        container: pod_name.to_string(),
        message: match container_name {
            Some(name) => format!("no container named '{}' found in pod '{}'", name, pod_name),
            None => format!("no containers found in pod '{}'", pod_name),
        },
    })?;

    let containerid =
        containerid
            .strip_prefix("containerd://")
            .ok_or_else(|| Error::UnexpectedOutput {
                command: "kubectl get pod".to_string(),
                message: format!(
                    "container ID does not have expected 'containerd://' prefix: {}",
                    containerid
                ),
            })?;
    Ok(String::from(containerid))
}

pub(crate) fn find_cgroup(containerdid: String) -> Result<PathBuf, Error> {
    let root = PathBuf::from("/sys/fs/cgroup");
    let containerdid = OsString::from(containerdid);
    match visit_dirs(&root, &containerdid) {
        Some(path) => Ok(path),
        None => Err(Error::ContainerNotFound {
            container: containerdid.to_string_lossy().into_owned(),
            message: "cgroup not found in /sys/fs/cgroup".to_string(),
        }),
    }
}

// one possible implementation of walking a directory from
// https://doc.rust-lang.org/std/fs/fn.read_dir.html
fn visit_dirs(dir: &Path, containerdid: &OsString) -> Option<PathBuf> {
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        if &entry.file_name() == containerdid {
            return Some(entry.path());
        }
        let path = entry.path();
        if path.is_dir() {
            if let Some(path) = visit_dirs(&path, containerdid) {
                return Some(path);
            }
        }
    }
    None
}

/// return any pid part of this cgroup
pub(crate) fn get_cgroup_pid(cgroup: &Path) -> Result<RawPid, Error> {
    let path = cgroup.join("cgroup.procs");
    let bytes = fs::read(&path).map_err(|source| Error::Io {
        path: path.clone(),
        source,
    })?;
    let pids = String::from_utf8(bytes).map_err(|_| Error::UnexpectedOutput {
        command: format!("reading {}", path.display()),
        message: "cgroup.procs contains non-UTF8 data".to_string(),
    })?;
    let pids = pids.splitn(2, '\n').collect::<Vec<&str>>()[0]; // first line
    let pid: u64 = u64::from_str(pids).map_err(|source| Error::InvalidPid {
        pid: pids.to_string(),
        runtime: "kubernetes",
        container: path.display().to_string(),
        source,
    })?;
    Ok(pid as RawPid)
}
