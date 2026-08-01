use rustix::io::Errno;
use rustix::process::Pid;
use rustix::thread::{LinkNameSpaceType, move_into_link_name_space};
use std::collections::HashSet;
use std::os::unix::prelude::*;
use std::path::PathBuf;
use thiserror::Error;

use crate::fsutil;
use crate::procfs;

#[derive(Debug, Error)]
pub(crate) enum NamespaceError {
    #[error("failed to read directory /proc/self/ns")]
    ListNamespaces(#[source] std::io::Error),
    #[error("failed to open namespace file '{path}'")]
    OpenNamespaceFile {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to set namespace '{name}'")]
    SetNamespace {
        name: &'static str,
        #[source]
        source: Errno,
    },
}

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

pub(crate) fn supported_namespaces() -> Result<HashSet<String>, NamespaceError> {
    let mut namespaces = HashSet::new();
    let names = fsutil::read_dir_names("/proc/self/ns").map_err(NamespaceError::ListNamespaces)?;
    for name in names {
        if let Ok(name) = name.into_string() {
            namespaces.insert(name);
        }
    }
    Ok(namespaces)
}

impl Kind {
    pub(crate) fn open(&'static self, pid: Pid) -> Result<Namespace, NamespaceError> {
        let path = self.path(pid);
        let file = fsutil::open_read(&path)
            .map_err(|source| NamespaceError::OpenNamespaceFile { path, source })?;
        Ok(Namespace { kind: self, file })
    }

    pub(crate) fn is_same(&self, pid: Pid) -> bool {
        let path = self.path(pid);
        match fsutil::read_link(path) {
            Ok(dest) => match fsutil::read_link(self.own_path()) {
                Ok(dest2) => dest == dest2,
                _ => false,
            },
            _ => false,
        }
    }
    fn path(&self, pid: Pid) -> PathBuf {
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
    file: OwnedFd,
}

impl Namespace {
    pub(crate) fn apply(&self) -> Result<(), NamespaceError> {
        move_into_link_name_space(self.file.as_fd(), None::<LinkNameSpaceType>).map_err(
            |source| NamespaceError::SetNamespace {
                name: self.kind.name,
                source,
            },
        )?;
        Ok(())
    }
}
