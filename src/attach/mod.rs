use crate::dotcntr;
use crate::fs;
use crate::ipc;
use crate::procfs;
use crate::result::Result;
use crate::user_namespace::IdMap;
use nix::unistd::{self, ForkResult, Pid, User};
use simple_error::{bail, try_with};
use std::fs::{create_dir_all, metadata};
use std::os::unix::prelude::*;

mod child;
mod parent;

pub struct AttachOptions {
    pub command: Option<String>,
    pub arguments: Vec<String>,
    pub container_name: String,
    pub container_types: Vec<Box<dyn container_pid::Container>>,
    pub effective_user: Option<User>,
}

pub fn attach(opts: &AttachOptions) -> Result<()> {
    let container_pid = match container_pid::lookup_container_pid(
        opts.container_name.as_str(),
        &opts.container_types,
    ) {
        Ok(pid) => Pid::from_raw(pid),
        Err(e) => bail!("{}", e),
    };

    let (uid_map, gid_map) = try_with!(
        IdMap::new_from_pid(container_pid),
        "failed to read usernamespace properties of {}",
        container_pid
    );

    let metadata = try_with!(
        metadata(procfs::get_path().join(container_pid.to_string())),
        "failed to container uid/gid"
    );

    let mut home = None;
    let mut effective_uid = None;
    let mut effective_gid = None;
    let container_uid = unistd::Uid::from_raw(uid_map.map_id_up(metadata.uid()));
    let container_gid = unistd::Gid::from_raw(gid_map.map_id_up(metadata.gid()));

    if let Some(ref passwd) = opts.effective_user {
        effective_uid = Some(passwd.uid);
        effective_gid = Some(passwd.gid);
        home = Some(passwd.dir.clone());
    }

    let process_status = try_with!(
        procfs::status(container_pid),
        "failed to get status of target process"
    );

    let dotcntr = try_with!(dotcntr::create(&process_status), "failed to setup /.cntr");

    let cntrfs = try_with!(
        fs::CntrFs::new(
            &fs::CntrMountOptions {
                prefix: "/",
                uid_map,
                gid_map,
                effective_uid,
                effective_gid,
            },
            Some(dotcntr),
        ),
        "cannot mount filesystem"
    );

    try_with!(
        create_dir_all("/var/lib/cntr"),
        "failed to create /var/lib/cntr"
    );

    let (parent_sock, child_sock) = try_with!(ipc::socket_pair(), "failed to set up ipc");

    let res = unsafe { unistd::fork() };
    match try_with!(res, "failed to fork") {
        ForkResult::Parent { child } => parent::run(child, &parent_sock, cntrfs),
        ForkResult::Child => {
            let child_opts = child::ChildOptions {
                command: opts.command.clone(),
                arguments: opts.arguments.clone(),
                mount_ready_sock: &child_sock,
                uid: container_uid,
                gid: container_gid,
                fs: cntrfs,
                process_status,
                home,
            };
            child::run(&child_opts)
        }
    }
}
