use anyhow::Context;
use nix::sched;
use nix::unistd;
use std::collections::HashSet;
use std::fs::{self, File};
use std::os::unix::prelude::*;
use std::path::PathBuf;

use crate::procfs;
use crate::result::Result;

pub(crate) const MOUNT: Kind = Kind { name: "mnt" };
pub(crate) const UTS: Kind = Kind { name: "uts" };
pub(crate) const USER: Kind = Kind { name: "user" };
pub(crate) const PID: Kind = Kind { name: "pid" };
pub(crate) const NET: Kind = Kind { name: "net" };
pub(crate) const CGROUP: Kind = Kind { name: "cgroup" };
pub(crate) const IPC: Kind = Kind { name: "ipc" };

pub(crate) struct Kind {
    pub(crate) name: &'static str,
}

pub(crate) fn supported_namespaces() -> Result<HashSet<String>> {
    let mut namespaces = HashSet::new();
    let entries = fs::read_dir(PathBuf::from("/proc/self/ns"))
        .context("failed to open directory /proc/self/ns")?;
    for entry in entries {
        let entry = entry.context("failed to read directory entry in /proc/self/ns")?;
        if let Ok(name) = entry.file_name().into_string() {
            namespaces.insert(name);
        }
    }
    Ok(namespaces)
}

impl Kind {
    pub(crate) fn open(&'static self, pid: unistd::Pid) -> Result<Namespace> {
        let buf = self.path(pid);
        let file = File::open(&buf)
            .with_context(|| format!("failed to open namespace file '{}'", buf.display()))?;
        Ok(Namespace { kind: self, file })
    }

    pub(crate) fn is_same(&self, pid: unistd::Pid) -> bool {
        let path = self.path(pid);
        match fs::read_link(path) {
            Ok(dest) => match fs::read_link(self.own_path()) {
                Ok(dest2) => dest == dest2,
                _ => false,
            },
            _ => false,
        }
    }
    fn path(&self, pid: unistd::Pid) -> PathBuf {
        procfs::get_path()
            .join(pid.to_string())
            .join("ns")
            .join(self.name)
    }

    fn own_path(&self) -> PathBuf {
        PathBuf::from("/proc/self/ns").join(self.name)
    }
}

pub(crate) struct Namespace {
    pub(crate) kind: &'static Kind,
    file: File,
}

impl Namespace {
    pub(crate) fn apply(&self) -> Result<()> {
        sched::setns(self.file.as_fd(), sched::CloneFlags::empty())
            .with_context(|| format!("failed to set namespace '{}'", self.kind.name))?;
        Ok(())
    }
}
