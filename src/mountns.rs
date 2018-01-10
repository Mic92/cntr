use file_utils::mkdir_p;
use fs::CntrFs;
use ipc;
use libc;
use mount_utils;
use namespace;
use nix::{sched, unistd};
use nix::sched::CloneFlags;
use std::ffi::OsStr;
use std::fs::{metadata, remove_dir};
use std::io;
use std::os::unix::prelude::*;
use std::path::PathBuf;
use tempdir::TempDir;
use types::{Error, Result};

pub struct MountNamespace {
    old_namespace: namespace::Namespace,
    mountpoint: PathBuf,
    temp_mountpoint: PathBuf,
}

const MOUNTS: &[&str] = &[
    "etc/passwd",
    "etc/group",
    "etc/resolv.conf",
    "etc/hosts",
    "etc/hostname",
    "etc/localtime",
    "etc/zoneinfo",
    "dev",
    "sys",
    "proc",
];

const CNTR_MOUNT_POINT: &str = "var/lib/cntr";

impl MountNamespace {
    fn new(old_namespace: namespace::Namespace) -> Result<MountNamespace> {
        let path = PathBuf::from("/tmp");
        tryfmt!(mkdir_p(&path), "failed to create /tmp");

        let mountpoint = tryfmt!(
            TempDir::new("cntrfs"),
            "failed to create temporary mountpoint"
        );

        let temp_mountpoint = tryfmt!(
            TempDir::new("cntrfs-temp"),
            "failed to create temporary mountpoint"
        );

        tryfmt!(
            sched::unshare(CloneFlags::CLONE_NEWNS),
            "failed to create mount namespace"
        );

        Ok(MountNamespace {
            old_namespace,
            mountpoint: mountpoint.into_path(),
            temp_mountpoint: temp_mountpoint.into_path(),
        })
    }

    fn send(self, sock: &ipc::Socket) -> Result<Self> {
        let res = {
            let message = &[
                self.mountpoint.as_os_str().as_bytes(),
                b"\0",
                self.temp_mountpoint.as_os_str().as_bytes(),
            ];
            sock.send(message, &[self.old_namespace.file()])
        };
        match res {
            Ok(_) => Ok(self),
            Err(e) => {
                self.cleanup();
                Err(e)
            }
        }
    }

    pub fn receive(sock: &ipc::Socket) -> Result<MountNamespace> {
        let (paths, mut fds) = tryfmt!(
            sock.receive((libc::PATH_MAX * 2) as usize, 2),
            "failed to receive mount namespace"
        );
        let paths: Vec<&[u8]> = paths.splitn(2, |c| *c == b'\0').collect();
        assert!(paths.len() == 2);

        let fd = fds.pop().unwrap();

        Ok(MountNamespace {
            old_namespace: namespace::MOUNT.namespace_from_file(fd),
            mountpoint: PathBuf::from(OsStr::from_bytes(paths[0])),
            temp_mountpoint: PathBuf::from(OsStr::from_bytes(paths[1])),
        })
    }

    pub fn cleanup(self) {
        if let Err(err) = self.old_namespace.apply() {
            warn!("failed to switch back to old mount namespace: {}", err);
            return;
        }

        if let Err(err) = remove_dir(&self.mountpoint) {
            warn!(
                "failed to cleanup mountpoint {:?}: {}",
                self.mountpoint,
                err
            );
        }

        if let Err(err) = remove_dir(&self.temp_mountpoint) {
            warn!(
                "failed to cleanup temporary mountpoint {:?}: {}",
                self.mountpoint,
                err
            );
        }
    }
}


pub fn setup_bindmounts(mounts: &[&str]) -> Result<()> {
    for m in mounts {
        let mountpoint_buf = PathBuf::from("/").join(m);
        let mountpoint = mountpoint_buf.as_path();
        let source_buf = PathBuf::from("/var/lib/cntr").join(m);
        let source = source_buf.as_path();

        let mountpoint_stat = match metadata(mountpoint) {
            Err(e) => {
                if e.kind() == io::ErrorKind::NotFound {
                    continue;
                }
                return tryfmt!(
                    Err(e),
                    "failed to get metadata of path {}",
                    mountpoint.display()
                );
            }
            Ok(data) => data,
        };

        let source_stat = match metadata(source) {
            Err(e) => {
                if e.kind() == io::ErrorKind::NotFound {
                    continue;
                }
                return tryfmt!(
                    Err(e),
                    "failed to get metadata of path {}",
                    source.display()
                );
            }
            Ok(data) => data,
        };

        if !((source_stat.is_file() && !mountpoint_stat.is_dir()) ||
                 (source_stat.is_dir() && mountpoint_stat.is_dir()))
        {
            continue;
        }

        let res = mount_utils::bind_mount(source, mountpoint);

        if res.is_err() {
            warn!("could not bind mount {:?}", mountpoint);
        }
    }
    Ok(())
}

pub fn setup(
    fs: &CntrFs,
    socket: &ipc::Socket,
    container_namespace: namespace::Namespace,
    mount_label: &Option<String>,
) -> Result<()> {
    let ns = tryfmt!(MountNamespace::new(container_namespace), "");

    tryfmt!(mount_utils::mount_private("/"), "unable to bind mount /");

    // prepare bind mounts
    tryfmt!(
        mount_utils::bind_mount("/", &ns.temp_mountpoint),
        "unable to move container mounts to new mountpoint"
    );
    tryfmt!(fs.mount(ns.mountpoint.as_path(), mount_label), "mount()");

    let ns = tryfmt!(ns.send(socket), "parent failed");

    tryfmt!(
        mkdir_p(&ns.mountpoint.join(CNTR_MOUNT_POINT)),
        "cannot create container mountpoint /{}",
        CNTR_MOUNT_POINT
    );

    tryfmt!(
        mount_utils::move_mounts(&ns.temp_mountpoint, &ns.mountpoint.join(CNTR_MOUNT_POINT)),
        "unable to move container mounts to new mountpoint"
    );

    tryfmt!(
        unistd::chdir(&ns.mountpoint),
        "failed to chdir to new mountpoint"
    );

    tryfmt!(
        unistd::chroot(&ns.mountpoint),
        "failed to chroot to new mountpoint"
    );

    tryfmt!(setup_bindmounts(MOUNTS), "failed to setup bind mounts");


    Ok(())
}
