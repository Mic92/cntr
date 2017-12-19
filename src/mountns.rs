use fs::CntrFs;
use namespace;
use nix::{mount, sched, unistd};
use nix::mount::MsFlags;
use nix::sched::CloneFlags;
use std::fs::{File, remove_dir, create_dir_all};
use std::io::Write;
use std::path::PathBuf;
use tempdir::TempDir;
use types::{Error, Result};

pub struct MountNamespace {
    old_namespace: namespace::Namespace,
    mountpoint: PathBuf,
    temp_mountpoint: PathBuf,
}

const READY_MSG: &[u8] = b"R";

const CNTR_MOUNT_POINT : &str = "var/lib/cntr";

impl MountNamespace {
    fn new(old_namespace: namespace::Namespace) -> Result<MountNamespace> {
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
            old_namespace: old_namespace,
            mountpoint: mountpoint.into_path(),
            temp_mountpoint: temp_mountpoint.into_path(),
        })
    }
}

impl Drop for MountNamespace {
    fn drop(&mut self) {

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

const NONE: Option<&'static [u8]> = None;

pub fn setup(
    fs: CntrFs,
    mut mount_ready_file: File,
    container_namespace: namespace::Namespace,
) -> Result<MountNamespace> {
    let ns = tryfmt!(MountNamespace::new(container_namespace), "");

    tryfmt!(
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
    tryfmt!(
        mount::mount(
            Some("/"),
            &ns.temp_mountpoint,
            NONE,
            MsFlags::MS_REC | MsFlags::MS_BIND,
            NONE,
        ),
        "unable to move container mounts to new mountpoint"
    );

    tryfmt!(fs.mount(ns.mountpoint.as_path()), "mount()");

    tryfmt!(mount_ready_file.write_all(READY_MSG), "parent failed");

    tryfmt!(create_dir_all(ns.mountpoint.join(CNTR_MOUNT_POINT)), "cannot create /{}", CNTR_MOUNT_POINT);

    tryfmt!(
        mount::mount(
            Some(&ns.temp_mountpoint),
            &ns.mountpoint.join(CNTR_MOUNT_POINT),
            NONE,
            MsFlags::MS_REC | MsFlags::MS_MOVE,
            NONE,
        ),
        "unable to move container mounts to new mountpoint"
    );


    for m in ["dev", "sys", "proc"].iter() {
        let mountpoint = &ns.mountpoint.join(m);
        tryfmt!(create_dir_all(mountpoint), "cannot create /{}", m);

        let res = mount::mount(
            Some(&PathBuf::from("/").join(m)),
            mountpoint,
            NONE,
            MsFlags::MS_REC | MsFlags::MS_BIND,
            NONE,
        );
        if res.is_err() {
            warn!("could not bind mount {:?}", mountpoint);
        }
    }

    tryfmt!(
        unistd::chdir(&ns.mountpoint),
        "failed to chdir to new mountpoint"
    );

    tryfmt!(
        unistd::chroot(&ns.mountpoint),
        "failed to chroot to new mountpoint"
    );

    Ok(ns)
}
