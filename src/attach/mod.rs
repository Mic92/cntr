use crate::ApparmorMode;
use crate::cgroup::CgroupError;
use crate::cmd::CmdError;
use crate::container::ContainerError;
use crate::container_setup::SetupError;
use crate::ipc::{self, IpcError};
use crate::lsm::LsmError;
use crate::namespace::NamespaceError;
use crate::passwd::User;
use crate::pty::PtyError;
use crate::syscalls::capability;
use crate::syscalls::process::{Fork, fork};
use idmap_helper::IdmapError;
use rustix::io::Errno;
use rustix::process::{getgid, getuid};
use std::path::PathBuf;
use std::process;
use thiserror::Error;

mod child;
mod idmap_helper;
mod parent;

#[derive(Debug, Error)]
pub(crate) enum AttachError {
    #[error(
        "Linux mount API is not available. cntr requires kernel 6.8+ with mount API support.\n\
         Please upgrade your kernel or use an older version of cntr with FUSE support."
    )]
    MountApiUnavailable,
    #[error("failed to lookup container '{container}'")]
    LookupContainer {
        container: String,
        #[source]
        source: ContainerError,
    },
    #[error("failed to create idmap helper for --effective-user")]
    IdmapHelper(#[from] IdmapError),
    #[error("failed to set up ipc")]
    Ipc(#[from] IpcError),
    #[error("failed to fork")]
    Fork(#[source] Errno),
    #[error("child did not send ready signal")]
    ChildNotReady,
    #[error("expected PTY fd from child, got none")]
    MissingPtyFd,
    #[error(transparent)]
    Pty(#[from] PtyError),
    #[error("failed to change cgroup")]
    Cgroup(#[from] CgroupError),
    #[error(transparent)]
    Cmd(#[from] CmdError),
    #[error(transparent)]
    Namespace(#[from] NamespaceError),
    #[error("the system has no support for mount namespaces")]
    MountNamespaceUnsupported,
    #[error("failed to create {path}")]
    CreateBaseDir {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to open container root at {path}")]
    OpenContainerRoot {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to unshare mount namespace")]
    UnshareMountNamespace(#[source] Errno),
    #[error("failed to make mounts private")]
    MakeMountsPrivate(#[source] Errno),
    #[error("failed to read /proc/mounts")]
    OpenProcMounts(#[source] std::io::Error),
    #[error("failed to mount tmpfs at {path}")]
    MountTmpfs {
        path: PathBuf,
        #[source]
        source: Errno,
    },
    #[error("failed to read container root directory")]
    ReadContainerRootDir(#[source] Errno),
    #[error(transparent)]
    Setup(#[from] SetupError),
    #[error("failed to inherit AppArmor profile")]
    Lsm(#[from] LsmError),
}

pub(crate) struct AttachOptions {
    pub(crate) command: Option<String>,
    pub(crate) arguments: Vec<String>,
    pub(crate) container_name: String,
    pub(crate) container_types: Vec<Box<dyn crate::container_pid::Container>>,
    pub(crate) effective_user: Option<User>,
    pub(crate) apparmor_mode: ApparmorMode,
}

pub(crate) fn attach(opts: &AttachOptions) -> Result<std::convert::Infallible, AttachError> {
    // Verify mount API capability - REQUIRED (no FUSE fallback)
    if !capability::has_mount_api() {
        return Err(AttachError::MountApiUnavailable);
    }

    // Lookup container and get its process status
    let process_status = crate::container::lookup_container(
        &opts.container_name,
        &opts.container_types,
        opts.apparmor_mode,
    )
    .map_err(|source| AttachError::LookupContainer {
        container: opts.container_name.clone(),
        source,
    })?;

    // Create idmap helper if --effective-user is specified
    // This creates a user namespace with the mapping for idmapped mounts
    let idmap_helper = if let Some(ref user) = opts.effective_user {
        let current_uid = getuid(); // Our actual UID (0 when running with sudo)
        let current_gid = getgid();
        let target_uid = user.uid; // Target UID for files on host
        let target_gid = user.gid;

        // IMPORTANT: Reverse mapping for idmapped mounts!
        // Map: target_uid (inside userns) → current_uid (outside userns)
        // This makes files owned by current_uid appear as owned by target_uid through the idmapped mount
        let helper =
            idmap_helper::IdmapHelper::new(target_uid, current_uid, target_gid, current_gid)?;

        Some(helper)
    } else {
        None
    };

    // Get userns FD and home dir if we have an idmap helper
    let userns_fd = idmap_helper.as_ref().map(|h| h.userns_fd());
    let effective_home = opts.effective_user.as_ref().map(|u| u.dir.clone());

    // Two-process dance for cross-namespace mount operations
    // Parent stays in host namespace, child assembles mount hierarchy
    let (parent_sock, child_sock) = ipc::socket_pair()?;

    match fork().map_err(AttachError::Fork)? {
        Fork::ParentOf(child) => {
            // Close child's socket in parent to ensure proper EOF detection
            drop(child_sock);
            // Keep idmap_helper alive for the duration of attach
            let result = parent::run(child, &process_status, &parent_sock);
            drop(idmap_helper);
            result
        }
        Fork::Child => {
            // Close parent's socket in child
            drop(parent_sock);
            let mut child_opts = child::ChildOptions {
                command: opts.command.clone(),
                arguments: opts.arguments.clone(),
                process_status,
                socket: &child_sock,
                userns_fd,
                effective_home,
            };
            // child::run returns Result<Infallible>, so can only return Err
            let Err(e) = child::run(&mut child_opts);
            eprintln!("attach child failed: {}", crate::errors::format_chain(&e));
            process::exit(1);
        }
    }
}
