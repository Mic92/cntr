use log::warn;
use nix::sys::signal::{self, Signal};
use nix::unistd;
use nix::unistd::{Gid, Uid};
use simple_error::{bail, try_with};
use std::env;
use std::ffi::CString;
use std::fs::File;
use std::os::unix::io::{AsRawFd, FromRawFd, IntoRawFd};
use std::os::unix::prelude::*;
use std::path::PathBuf;
use std::process;

use crate::capabilities;
use crate::cgroup;
use crate::cmd::Cmd;
use crate::ipc;
use crate::lsm;
use crate::namespace;
use crate::procfs::ProcStatus;
use crate::pty;
use crate::result::Result;
use crate::syscalls::mount_api::{AT_FDCWD, AT_RECURSIVE, MountFd, OPEN_TREE_CLONE};
use nix::sched::{CloneFlags, unshare};

/// Options for child process
pub(crate) struct ChildOptions<'a> {
    pub command: Option<String>,
    pub arguments: Vec<String>,
    pub process_status: ProcStatus,
    pub socket: &'a ipc::Socket,
    pub home: Option<PathBuf>,
    pub uid: Uid,
    pub gid: Gid,
}

/// Child process logic for mount API attach (T018)
///
/// The child assembles a mount hierarchy where:
/// - / = host filesystem (with all host mounts)
/// - /var/lib/cntr = tmpfs overlay with container entries bind-mounted
/// - /var/lib/cntr/.exec.sock = daemon socket (on tmpfs)
///
/// Steps:
/// 1. Read LSM profile and move to container's cgroup
/// 2. Prepare command to execute
/// 3. Detect and open namespaces
/// 4. Assemble mount hierarchy:
///    - Resolve container's root path via /proc/<pid>/root (handles chroot)
///    - Create private mount namespace
///    - Create tmpfs at /var/lib/cntr
///    - Enter container's mount namespace
///    - Capture each container entry with open_tree() (includes submounts)
///    - Return to parent namespace
///    - Attach captured trees to /var/lib/cntr/*
/// 5. Enter other container namespaces (USER, NET, PID, IPC, UTS, CGROUP)
/// 6. Set UID/GID and drop capabilities
/// 7. Create daemon socket and setup PTY
/// 8. Execute the command
pub(crate) fn run(options: &ChildOptions) -> Result<()> {
    // Step 1: Read LSM profile before entering namespaces
    let lsm_profile = try_with!(
        lsm::read_profile(options.process_status.global_pid),
        "failed to get lsm profile"
    );

    let mount_label = if let Some(ref p) = lsm_profile {
        try_with!(
            p.mount_label(options.process_status.global_pid),
            "failed to read mount options"
        )
    } else {
        None
    };

    // Step 2: Move to container's cgroup
    try_with!(
        cgroup::move_to(unistd::getpid(), options.process_status.global_pid),
        "failed to change cgroup"
    );

    // Step 3: Prepare command to execute
    let cmd = Cmd::new(
        options.command.clone(),
        options.arguments.clone(),
        options.process_status.global_pid,
        options.home.clone(),
    )?;

    // Step 4: Detect and open namespaces
    let supported_namespaces = try_with!(
        namespace::supported_namespaces(),
        "failed to list namespaces"
    );

    if !supported_namespaces.contains(namespace::MOUNT.name) {
        bail!("the system has no support for mount namespaces")
    };

    let mut other_namespaces = Vec::new();
    let other_kinds = &[
        namespace::UTS,
        namespace::CGROUP,
        namespace::PID,
        namespace::NET,
        namespace::IPC,
        namespace::USER,
    ];

    for kind in other_kinds {
        if !supported_namespaces.contains(kind.name) {
            continue;
        }
        if kind.is_same(options.process_status.global_pid) {
            continue;
        }

        other_namespaces.push(try_with!(
            kind.open(options.process_status.global_pid),
            "failed to open {} namespace",
            kind.name
        ));
    }

    // Step 5: Assemble mount hierarchy
    // Goal: / = host filesystem, /var/lib/cntr = tmpfs with container entries mounted
    //
    // Strategy: Create tmpfs at /var/lib/cntr, use open_tree() to capture container
    // entries (with their submounts) from container namespace, attach to tmpfs

    // Resolve container's root path (handles chroot containers)
    // For chrooted processes, /proc/<pid>/root links to the chroot directory
    let proc_root_path = format!("/proc/{}/root", options.process_status.global_pid);
    let container_root_path = try_with!(
        std::fs::read_link(&proc_root_path),
        "failed to read container root path from {}",
        proc_root_path
    );

    // Create private mount namespace in parent
    try_with!(
        unshare(CloneFlags::CLONE_NEWNS),
        "failed to unshare mount namespace"
    );

    // Save our own mount namespace FD (with tmpfs)
    let our_mount_ns = try_with!(
        namespace::MOUNT.open(unistd::getpid()),
        "failed to open our own mount namespace"
    );

    // Create tmpfs at /var/lib/cntr (for socket and mount points)
    try_with!(
        std::fs::create_dir_all("/var/lib/cntr"),
        "failed to create /var/lib/cntr"
    );
    try_with!(
        nix::mount::mount(
            Some("tmpfs"),
            "/var/lib/cntr",
            Some("tmpfs"),
            nix::mount::MsFlags::empty(),
            None::<&str>
        ),
        "failed to mount tmpfs at /var/lib/cntr"
    );

    // Enter container's mount namespace to capture trees with submounts
    let container_mount_namespace = try_with!(
        namespace::MOUNT.open(options.process_status.global_pid),
        "could not access container mount namespace"
    );
    try_with!(
        container_mount_namespace.apply(),
        "failed to enter container mount namespace"
    );

    // Capture each container root entry with open_tree()
    let mut captured_trees = Vec::new();
    for entry in try_with!(
        std::fs::read_dir(&container_root_path),
        "failed to read container root at {}",
        container_root_path.display()
    ) {
        let entry = try_with!(entry, "failed to read directory entry");
        let file_name = entry.file_name();
        let file_name_str = file_name.to_string_lossy();

        // Skip special directories
        if file_name_str == "." || file_name_str == ".." {
            continue;
        }

        let source = entry.path();
        let source_cstr = try_with!(
            CString::new(source.to_str().unwrap()),
            "failed to create CString for {}",
            source.display()
        );

        // Capture this entry's tree (includes all submounts)
        match MountFd::open_tree_at(&source_cstr, OPEN_TREE_CLONE | AT_RECURSIVE) {
            Ok(tree_fd) => {
                captured_trees.push((file_name, tree_fd));
            }
            Err(e) => {
                warn!("Failed to capture tree for {:?}: {}", source, e);
            }
        }
    }

    // Return to our own mount namespace (with tmpfs)
    try_with!(
        our_mount_ns.apply(),
        "failed to return to our mount namespace"
    );

    // Attach each captured tree to /var/lib/cntr
    for (file_name, tree_fd) in captured_trees {
        let target = PathBuf::from("/var/lib/cntr").join(&file_name);

        // Create mount point
        let is_dir = file_name.to_string_lossy() != ".exec.sock"; // Assume directories
        if is_dir {
            let _ = std::fs::create_dir(&target);
        } else {
            let _ = std::fs::File::create(&target);
        }

        let target_cstr = try_with!(
            CString::new(target.to_str().unwrap()),
            "failed to create CString for {}",
            target.display()
        );

        if let Err(e) = tree_fd.attach_to(AT_FDCWD, &target_cstr, 0) {
            warn!("Failed to attach tree to {:?}: {}", target, e);
        }
    }

    // Apply mount label if needed
    if let Some(label) = mount_label {
        // TODO: Apply mount label using mount_setattr if needed
        // For now, we skip this as it's primarily for SELinux contexts
        let _ = label; // Silence unused warning
    }

    // Step 6: Enter other container namespaces
    // Check if setgroups is already denied (happens in nested user namespaces)
    let setgroups_denied = std::fs::read_to_string("/proc/self/setgroups")
        .map(|s| s.trim() == "deny")
        .unwrap_or(false);

    let dropped_groups = if supported_namespaces.contains(namespace::USER.name) && !setgroups_denied
    {
        unistd::setgroups(&[]).is_ok()
    } else {
        setgroups_denied // Already denied, so consider it "dropped"
    };

    for ns in other_namespaces {
        try_with!(ns.apply(), "failed to apply namespace");
    }

    // Step 7: Set UID/GID
    if supported_namespaces.contains(namespace::USER.name) {
        // Only try to set groups if not already denied
        if !setgroups_denied
            && let Err(e) = unistd::setgroups(&[])
            && !dropped_groups
        {
            try_with!(Err(e), "could not set groups");
        }
        try_with!(unistd::setgid(options.gid), "could not set group id");
        try_with!(unistd::setuid(options.uid), "could not set user id");
    }

    // Step 8: Drop capabilities
    try_with!(
        capabilities::drop(options.process_status.effective_capabilities),
        "failed to apply capabilities"
    );

    // Step 9: Create daemon socket in the tmpfs overlay
    // The socket lives at /var/lib/cntr/.exec.sock on the tmpfs (writable)
    let daemon_sock = try_with!(
        crate::daemon::DaemonSocket::bind(options.process_status.clone()),
        "failed to create daemon socket"
    );

    // Step 10: Setup PTY
    let pty_master = try_with!(pty::open_ptm(), "open pty master");
    try_with!(pty::attach_pts(&pty_master), "failed to setup pty master");

    // Send ready signal + PTY fd + daemon socket fd to parent
    let ready_msg = b"R";
    let pty_file = unsafe { File::from_raw_fd(pty_master.as_raw_fd()) };
    let daemon_file = unsafe { File::from_raw_fd(daemon_sock.as_raw_fd()) };
    let res = options
        .socket
        .send(&[ready_msg], &[&pty_file, &daemon_file]);
    let _ = pty_file.into_raw_fd();
    let _ = daemon_file.into_raw_fd();
    try_with!(
        res,
        "failed to send ready signal, pty fd, and daemon socket fd to parent"
    );

    // Step 11: Change to /var/lib/cntr
    if let Err(e) = env::set_current_dir("/var/lib/cntr") {
        warn!("failed to change directory to /var/lib/cntr: {}", e);
    }

    // Step 12: Inherit LSM profile
    if let Some(profile) = lsm_profile {
        try_with!(profile.inherit_profile(), "failed to inherit lsm profile");
    }

    // Step 13: Execute the command
    let status = cmd.run()?;
    if let Some(signum) = status.signal() {
        let signal = try_with!(
            Signal::try_from(signum),
            "invalid signal received: {}",
            signum
        );
        try_with!(
            signal::kill(unistd::getpid(), signal),
            "failed to send signal {:?} to own pid",
            signal
        );
    }
    if let Some(code) = status.code() {
        process::exit(code);
    }
    eprintln!(
        "BUG! command exited successfully, \
         but was neither terminated by a signal nor has an exit code"
    );
    process::exit(1);
}
