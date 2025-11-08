use crate::container::ContainerContext;
use crate::ipc;
use crate::result::Result;
use crate::syscalls::capability;
use anyhow::{Context, bail};
use nix::unistd::{self, ForkResult, User};

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
    // Verify mount API capability - REQUIRED (no FUSE fallback)
    if !capability::has_mount_api() {
        bail!(
            "Linux mount API is not available. cntr requires kernel 6.8+ with mount API support.\n\
             Please upgrade your kernel or use an older version of cntr with FUSE support."
        );
    }

    // Lookup container and get its context
    let ctx = ContainerContext::lookup(&opts.container_name, &opts.container_types)?;

    let home = opts
        .effective_user
        .as_ref()
        .map(|passwd| passwd.dir.clone());

    // Two-process dance for cross-namespace mount operations
    // Parent stays in host namespace, child assembles mount hierarchy
    let (parent_sock, child_sock) = ipc::socket_pair().context("failed to set up ipc")?;

    let res = unsafe { unistd::fork() };
    match res.context("failed to fork")? {
        ForkResult::Parent { child } => parent::run(child, &ctx.process_status, &parent_sock),
        ForkResult::Child => {
            let child_opts = child::ChildOptions {
                command: opts.command.clone(),
                arguments: opts.arguments.clone(),
                process_status: ctx.process_status,
                socket: &child_sock,
                home,
                uid: ctx.uid,
                gid: ctx.gid,
            };
            child::run(&child_opts)
        }
    }
}
