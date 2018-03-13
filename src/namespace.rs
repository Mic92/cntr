use nix::sched;
use nix::unistd;
use std::collections::HashSet;
use std::fs::{self, File};
use std::os::unix::prelude::*;
use std::path::PathBuf;
use types::{Error, Result};

pub const MOUNT: Kind = Kind { name: "mnt" };
pub const UTS: Kind = Kind { name: "uts" };
pub const USER: Kind = Kind { name: "user" };
pub const PID: Kind = Kind { name: "pid" };
pub const NET: Kind = Kind { name: "net" };
pub const CGROUP: Kind = Kind { name: "cgroup" };
pub const IPC: Kind = Kind { name: "ipc" };

pub static ALL: &'static [Kind] = &[UTS, CGROUP, PID, NET, IPC, MOUNT, USER];

pub struct Kind {
    pub name: &'static str,
}

pub fn supported_namespaces() -> Result<HashSet<String>> {
    let mut namespaces = HashSet::new();
    let entries = tryfmt!(
        fs::read_dir(PathBuf::from("/proc/self/ns")),
        "failed to open directory /proc/self/ns"
    );
    for entry in entries {
        let entry = tryfmt!(entry, "failed to read directory /proc/self/ns");
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
        let file = tryfmt!(File::open(path), "failed to open namespace file '{}'", path);
        Ok(Namespace { kind: self, file })
    }

    pub fn namespace_from_file(&'static self, file: File) -> Namespace {
        Namespace { kind: self, file }
    }

    pub fn is_same(&self, pid: unistd::Pid) -> bool {
        let path = self.path(pid);
        let path2 = self.path(unistd::getpid());
        match fs::read_link(path) {
            Ok(dest) => {
                match fs::read_link(path2) {
                    Ok(dest2) => dest == dest2,
                    _ => false,
                }
            }
            _ => false,
        }
    }
    fn path(&self, pid: unistd::Pid) -> PathBuf {
        let mut buf = PathBuf::from("/proc/");
        buf.push(pid.to_string());
        buf.push("ns");
        buf.push(self.name);
        buf
    }
}


pub struct Namespace {
    pub kind: &'static Kind,
    file: File,
}

impl Namespace {
    pub fn apply(&self) -> Result<()> {
        tryfmt!(
            sched::setns(self.file.as_raw_fd(), sched::CloneFlags::empty()),
            "setns"
        );
        Ok(())
    }
    pub fn file(&self) -> &File {
        &self.file
    }
}
