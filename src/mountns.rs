use log::warn;
use nix::mount::MsFlags;
use nix::sched::CloneFlags;
use nix::{cmsg_space, mount, sched, unistd};
use simple_error::try_with;
use std::fs;
use std::io;
use std::os::unix::prelude::*;
use std::path::PathBuf;
use std::{
    ffi::OsStr,
    fs::{set_permissions, Permissions},
};

use crate::fs::CntrFs;
use crate::ipc;
use crate::namespace;
use crate::result::Result;
use crate::tmp;

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
        let mountpoint = try_with!(tmp::tempdir(), "failed to create temporary mountpoint");
        try_with!(
            set_permissions(mountpoint.path(), Permissions::from_mode(0o755)),
            "cannot change permissions of '{}'",
            mountpoint.path().display()
        );

        let temp_mountpoint = try_with!(tmp::tempdir(), "failed to create temporary mountpoint");
        try_with!(
            set_permissions(temp_mountpoint.path(), Permissions::from_mode(0o755)),
            "cannot change permissions of '{}'",
            temp_mountpoint.path().display()
        );

        try_with!(
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
        let mut cmsgspace = cmsg_space!([RawFd; 2]);
        let (paths, mut fds) = try_with!(
            sock.receive((libc::PATH_MAX * 2) as usize, &mut cmsgspace),
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

        if let Err(err) = fs::remove_dir(&self.mountpoint) {
            warn!(
                "failed to cleanup mountpoint {:?}: {}",
                self.mountpoint, err
            );
        }

        if let Err(err) = fs::remove_dir(&self.temp_mountpoint) {
            warn!(
                "failed to cleanup temporary mountpoint {:?}: {}",
                self.mountpoint, err
            );
        }
    }
}

const NONE: Option<&'static [u8]> = None;

pub fn setup_bindmounts(mounts: &[&str]) -> Result<()> {
    for m in mounts {
        let mountpoint_buf = PathBuf::from("/").join(m);
        let mountpoint = mountpoint_buf.as_path();
        let source_buf = PathBuf::from("/var/lib/cntr").join(m);
        let source = source_buf.as_path();

        let mountpoint_stat = match fs::metadata(mountpoint) {
            Err(e) => {
                if e.kind() == io::ErrorKind::NotFound {
                    continue;
                }
                return try_with!(
                    Err(e),
                    "failed to get metadata of path {}",
                    mountpoint.display()
                );
            }
            Ok(data) => data,
        };

        let source_stat = match fs::metadata(source) {
            Err(e) => {
                if e.kind() == io::ErrorKind::NotFound {
                    continue;
                }
                return try_with!(
                    Err(e),
                    "failed to get metadata of path {}",
                    source.display()
                );
            }
            Ok(data) => data,
        };

        #[allow(clippy::suspicious_operation_groupings)]
        if !((source_stat.is_file() && !mountpoint_stat.is_dir())
            || (source_stat.is_dir() && mountpoint_stat.is_dir()))
        {
            continue;
        }

        let res = mount::mount(
            Some(source),
            mountpoint,
            NONE,
            MsFlags::MS_REC | MsFlags::MS_BIND,
            NONE,
        );

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
    try_with!(
        mkdir_p(&CNTR_MOUNT_POINT),
        "cannot create container mountpoint /{}",
        CNTR_MOUNT_POINT
    );

    let ns = MountNamespace::new(container_namespace)?;

    try_with!(
        mount::mount(
            Some("none"),
            "/",
            NONE,
            MsFlags::MS_REC | MsFlags::MS_PRIVATE,
            NONE,
        ),
        "unable to bind mount /"
    );

    // prepare bind mounts
    try_with!(
        mount::mount(
            Some("/"),
            &ns.temp_mountpoint,
            NONE,
            MsFlags::MS_REC | MsFlags::MS_BIND,
            NONE,
        ),
        "unable to move container mounts to new mountpoint"
    );
    try_with!(fs.mount(ns.mountpoint.as_path(), mount_label), "mount()");

    let ns = try_with!(ns.send(socket), "parent failed");

    try_with!(
        mount::mount(
            Some(&ns.temp_mountpoint),
            &ns.mountpoint.join(CNTR_MOUNT_POINT),
            NONE,
            MsFlags::MS_REC | MsFlags::MS_MOVE,
            NONE,
        ),
        "unable to move container mounts to new mountpoint"
    );

    try_with!(
        unistd::chdir(&ns.mountpoint),
        "failed to chdir to new mountpoint"
    );

    try_with!(
        unistd::chroot(&ns.mountpoint),
        "failed to chroot to new mountpoint"
    );

    try_with!(setup_bindmounts(MOUNTS), "failed to setup bind mounts");

    Ok(())
}
