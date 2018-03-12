use capabilities;
use container;
use fs;
use ipc;
use nix::unistd::{self, ForkResult};
use pwd;
use std::fs::metadata;
use std::os::unix::prelude::*;
use types::{Error, Result};
use user_namespace::IdMap;
use void::Void;

mod child;
mod parent;

pub struct AttachOptions {
    pub container_name: String,
    pub container_types: Vec<Box<container::Container>>,
    pub effective_user: Option<pwd::Passwd>,
}

pub fn attach(opts: &AttachOptions) -> Result<Void> {
    let container_pid =
        tryfmt!(
            container::lookup_container_pid(opts.container_name.as_str(), &opts.container_types),
            ""
        );

    let (uid_map, gid_map) = tryfmt!(
        IdMap::new_from_pid(container_pid),
        "failed to read usernamespace properties of {}",
        container_pid
    );

    let metadata = tryfmt!(
        metadata(format!("/proc/{}", container_pid)),
        "failed to container uid/gid"
    );

    let mut home = None;
    let mut effective_uid = None;
    let mut effective_gid = None;
    let container_uid = unistd::Uid::from_raw(uid_map.map_id_up(metadata.uid()));
    let container_gid = unistd::Gid::from_raw(gid_map.map_id_up(metadata.gid()));

    if let Some(ref passwd) = opts.effective_user {
        effective_uid = Some(passwd.pw_uid);
        effective_gid = Some(passwd.pw_gid);
        home = Some(passwd.pw_dir.as_ref());
    }

    let cntrfs = tryfmt!(
        fs::CntrFs::new(&fs::CntrMountOptions {
            prefix: "/",
            splice_read: cfg!(feature = "splice_read"),
            splice_write: false,
            uid_map: uid_map,
            gid_map: gid_map,
            effective_uid: effective_uid,
            effective_gid: effective_gid,
        }),
        "cannot mount filesystem"
    );

    let (parent_sock, child_sock) = tryfmt!(ipc::socket_pair(), "failed to set up ipc");

    match tryfmt!(unistd::fork(), "failed to fork") {
        ForkResult::Parent { child } => parent::run(child, &parent_sock, cntrfs),
        ForkResult::Child => {
            let opts = child::ChildOptions {
                container_pid: container_pid,
                mount_ready_sock: &child_sock,
                uid: container_uid,
                gid: container_gid,
                fs: cntrfs,
                home: home,
            };
            child::run(&opts)
        }
    }
}
