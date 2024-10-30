use nix::sched;
use nix::unistd;
use simple_error::try_with;
use std::collections::HashSet;
use std::fs::{self, File};
use std::os::unix::prelude::*;
use std::path::PathBuf;

use crate::procfs;
use crate::result::Result;

pub const MOUNT: Kind = Kind { name: "mnt" };
pub const UTS: Kind = Kind { name: "uts" };
pub const USER: Kind = Kind { name: "user" };
pub const PID: Kind = Kind { name: "pid" };
pub const NET: Kind = Kind { name: "net" };
pub const CGROUP: Kind = Kind { name: "cgroup" };
pub const IPC: Kind = Kind { name: "ipc" };

pub static ALL: &[Kind] = &[UTS, CGROUP, PID, NET, IPC, MOUNT, USER];

pub struct Kind {
    pub name: &'static str,
}

pub fn supported_namespaces() -> Result<HashSet<String>> {
    let mut namespaces = HashSet::new();
    let entries = try_with!(
        fs::read_dir(PathBuf::from("/proc/self/ns")),
        "failed to open directory /proc/self/ns"
    );
    for entry in entries {
        let entry = try_with!(entry, "failed to read directory /proc/self/ns");
        if let Ok(name) = entry.file_name().into_string() {
            namespaces.insert(name);
        }
    }
    Ok(namespaces)
}

impl Kind {
    pub fn open(&'static self, pid: unistd::Pid) -> Result<Namespace> {
        let buf = self.path(pid);
        let path = buf.to_str().unwrap();
        let file = try_with!(File::open(path), "failed to open namespace file '{}'", path);
        Ok(Namespace { kind: self, file })
    }

    pub fn namespace_from_file(&'static self, file: File) -> Namespace {
        Namespace { kind: self, file }
    }

    pub fn is_same(&self, pid: unistd::Pid) -> bool {
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

pub struct Namespace {
    pub kind: &'static Kind,
    file: File,
}

impl Namespace {
    pub fn apply(&self) -> Result<()> {
        try_with!(
            sched::setns(self.file.as_fd(), sched::CloneFlags::empty()),
            "setns"
        );
        Ok(())
    }
    pub fn file(&self) -> &File {
        &self.file
    }
}
