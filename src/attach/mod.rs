use crate::container::ContainerContext;
use crate::ipc;
use crate::result::Result;
use crate::syscalls::capability;
use anyhow::{Context, bail};
use nix::unistd::{self, ForkResult, User};
use std::os::unix::io::AsRawFd;

mod child;
mod idmap_helper;
mod parent;

pub(crate) struct AttachOptions {
    pub(crate) command: Option<String>,
    pub(crate) arguments: Vec<String>,
    pub(crate) container_name: String,
    pub(crate) container_types: Vec<Box<dyn container_pid::Container>>,
    pub(crate) effective_user: Option<User>,
}

pub(crate) fn attach(opts: &AttachOptions) -> Result<()> {
    // Verify mount API capability - REQUIRED (no FUSE fallback)
    if !capability::has_mount_api() {
        bail!(
            "Linux mount API is not available. cntr requires kernel 6.8+ with mount API support.\n\
             Please upgrade your kernel or use an older version of cntr with FUSE support."
        );
    }

    // Lookup container and get its context
    let ctx = ContainerContext::lookup(&opts.container_name, &opts.container_types)?;

    // Create idmap helper if --effective-user is specified
    // This creates a user namespace with the mapping for idmapped mounts
    let idmap_helper = if let Some(ref user) = opts.effective_user {
        let current_uid = unistd::getuid(); // Our actual UID (0 when running with sudo)
        let current_gid = unistd::getgid();
        let target_uid = user.uid; // Target UID for files on host
        let target_gid = user.gid;

        // IMPORTANT: Reverse mapping for idmapped mounts!
        // Map: target_uid (inside userns) → current_uid (outside userns)
        // This makes files owned by current_uid appear as owned by target_uid through the idmapped mount
        let helper =
            idmap_helper::IdmapHelper::new(target_uid, current_uid, target_gid, current_gid)
                .context("failed to create idmap helper for --effective-user")?;

        Some(helper)
    } else {
        None
    };

    // Get userns FD and home dir if we have an idmap helper
    let userns_fd = idmap_helper.as_ref().map(|h| h.userns_fd().as_raw_fd());
    let effective_home = opts.effective_user.as_ref().map(|u| u.dir.clone());

    // Two-process dance for cross-namespace mount operations
    // Parent stays in host namespace, child assembles mount hierarchy
    let (parent_sock, child_sock) = ipc::socket_pair().context("failed to set up ipc")?;

    let res = unsafe { unistd::fork() };
    match res.context("failed to fork")? {
        ForkResult::Parent { child } => {
            // Keep idmap_helper alive for the duration of attach
            let result = parent::run(child, &ctx.process_status, &parent_sock);
            drop(idmap_helper);
            result
        }
        ForkResult::Child => {
            let child_opts = child::ChildOptions {
                command: opts.command.clone(),
                arguments: opts.arguments.clone(),
                process_status: ctx.process_status,
                socket: &child_sock,
                userns_fd,
                effective_home,
                uid: ctx.uid,
                gid: ctx.gid,
            };
            child::run(&child_opts)
        }
    }
}
