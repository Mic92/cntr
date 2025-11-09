use anyhow::{Context, bail};
use log::{debug, warn};
use nix::unistd;
use nix::unistd::{Gid, Uid};
use std::env;
use std::ffi::CString;
use std::os::unix::io::{BorrowedFd, RawFd};
use std::os::unix::prelude::*;
use std::path::{Path, PathBuf};

use crate::capabilities;
use crate::cgroup;
use crate::cmd::Cmd;
use crate::ipc;
use crate::lsm;
use crate::namespace;
use crate::paths;
use crate::procfs::ProcStatus;
use crate::pty;
use crate::result::Result;
use crate::syscalls::mount_api::{AT_RECURSIVE, MountFd, OPEN_TREE_CLONE};
use nix::sched::{CloneFlags, unshare};

/// Options for child process
pub(crate) struct ChildOptions<'a> {
    pub(crate) command: Option<String>,
    pub(crate) arguments: Vec<String>,
    pub(crate) process_status: ProcStatus,
    pub(crate) socket: &'a ipc::Socket,
    pub(crate) userns_fd: Option<RawFd>,
    pub(crate) effective_home: Option<PathBuf>,
    pub(crate) uid: Uid,
    pub(crate) gid: Gid,
}

/// Apply idmapped mounts to all supported filesystems
///
/// This makes all files created on the host appear as owned by the effective user.
/// Requires kernel 5.12+ and --effective-user option.
fn apply_idmapped_mounts(userns_fd: BorrowedFd, base_dir: &Path) -> Result<()> {
    use std::io::BufRead;

    // Read /proc/mounts to get all mount points
    let mounts_file = std::fs::File::open("/proc/mounts").context("failed to open /proc/mounts")?;
    let reader = std::io::BufReader::new(mounts_file);

    // Skip virtual/special filesystems that don't support idmapped mounts
    let skip_fstypes = [
        "proc",
        "sysfs",
        "devtmpfs",
        "devpts",
        "cgroup",
        "cgroup2",
        "securityfs",
        "debugfs",
        "tracefs",
        "pstore",
        "efivarfs",
        "mqueue",
        "hugetlbfs",
        "autofs",
        "fusectl",
        "configfs",
        "rpc_pipefs",
        "binfmt_misc",
        "overlay",
    ];

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => continue,
        };

        // Parse: device mountpoint fstype options
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 3 {
            continue;
        }

        let mount_point = parts[1];
        let fstype = parts[2];

        // Skip virtual filesystems
        if skip_fstypes.contains(&fstype) {
            continue;
        }

        // Skip the base_dir itself (we'll mount container stuff there)
        if Path::new(mount_point).starts_with(base_dir) {
            continue;
        }

        // Try to apply idmap to this mount
        let mount_cstr = match CString::new(mount_point) {
            Ok(c) => c,
            Err(_) => continue,
        };

        // Clone the mount with open_tree
        let tree = match MountFd::open_tree_at(&mount_cstr, OPEN_TREE_CLONE | AT_RECURSIVE) {
            Ok(t) => t,
            Err(e) => {
                warn!("Failed to open_tree {}: {}", mount_point, e);
                continue;
            }
        };

        // Apply idmap
        if let Err(e) = tree.apply_idmap(userns_fd) {
            warn!(
                "Failed to apply idmap to {} ({}): {}",
                mount_point, fstype, e
            );
            continue;
        }

        // Move back to original location
        if let Err(e) = tree.attach_to(None, &mount_cstr, 0) {
            warn!("Failed to attach idmapped {} back: {}", mount_point, e);
            continue;
        }

        debug!("Applied idmap to {} ({})", mount_point, fstype);
    }

    Ok(())
}

/// Child process logic for mount API attach
///
/// The child assembles a mount hierarchy where:
/// - / = host filesystem (with all host mounts)
/// - {base_dir} = tmpfs overlay with container entries bind-mounted
/// - {base_dir}/.exec.sock = daemon socket (on tmpfs)
///
/// Steps:
/// 1. Read LSM profile and move to container's cgroup
/// 2. Prepare command to execute
/// 3. Detect and open namespaces
/// 4. Assemble mount hierarchy:
///    - Resolve container's root path via /proc/<pid>/root (handles chroot)
///    - Create private mount namespace
///    - Create tmpfs at {base_dir}
///    - Enter container's mount namespace
///    - Capture each container entry with open_tree() (includes submounts)
///    - Return to parent namespace
///    - Attach captured trees to {base_dir}/*
/// 5. Enter other container namespaces (USER, NET, PID, IPC, UTS, CGROUP)
/// 6. Set UID/GID and drop capabilities
/// 7. Create daemon socket and setup PTY
/// 8. Execute the command
pub(crate) fn run(options: &ChildOptions) -> Result<()> {
    // Step 1: Read LSM profile before entering namespaces
    let lsm_profile = lsm::read_profile(options.process_status.global_pid)
        .context("failed to get lsm profile")?;

    let mount_label = if let Some(ref p) = lsm_profile {
        p.mount_label(options.process_status.global_pid)
            .context("failed to read mount options")?
    } else {
        None
    };

    // Step 2: Move to container's cgroup
    cgroup::move_to(unistd::getpid(), options.process_status.global_pid)
        .context("failed to change cgroup")?;

    // Step 3: Prepare command to execute
    let cmd = Cmd::new(
        options.command.clone(),
        options.arguments.clone(),
        options.process_status.global_pid,
        options.effective_home.clone(),
    )?;

    // Step 4: Detect and open namespaces
    let supported_namespaces =
        namespace::supported_namespaces().context("failed to list namespaces")?;

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

        other_namespaces.push(
            kind.open(options.process_status.global_pid)
                .with_context(|| format!("failed to open {} namespace", kind.name))?,
        );
    }

    // Step 5: Assemble mount hierarchy
    // Goal: / = host filesystem, {base_dir} = tmpfs with container entries mounted
    //
    // Strategy: Create tmpfs at {base_dir}, use open_tree() to capture container
    // entries (with their submounts) from container namespace, attach to tmpfs

    let base_dir = paths::get_base_dir();

    // Create base_dir BEFORE entering any namespaces
    // This ensures the directory exists in the host namespace
    std::fs::create_dir_all(&base_dir)
        .with_context(|| format!("failed to create {}", base_dir.display()))?;

    // Resolve container's root path (handles chroot containers)
    // For chrooted processes, /proc/<pid>/root links to the chroot directory
    let proc_root_path = format!("/proc/{}/root", options.process_status.global_pid);
    let container_root_path = std::fs::read_link(&proc_root_path)
        .with_context(|| format!("failed to read container root path from {}", proc_root_path))?;

    // Create private mount namespace
    unshare(CloneFlags::CLONE_NEWNS).context("failed to unshare mount namespace")?;

    // Make all mounts private (required before applying idmap)
    nix::mount::mount(
        None::<&str>,
        "/",
        None::<&str>,
        nix::mount::MsFlags::MS_REC | nix::mount::MsFlags::MS_PRIVATE,
        None::<&str>,
    )
    .context("failed to make mounts private")?;

    // Apply idmapped mount to all supported filesystems if --effective-user was specified
    // This makes all files created on the host appear as owned by the effective user
    if let Some(userns_fd) = options.userns_fd {
        let userns_borrowed = unsafe { BorrowedFd::borrow_raw(userns_fd) };
        apply_idmapped_mounts(userns_borrowed, &base_dir)
            .context("failed to apply idmapped mounts")?;
    }

    // Save our own mount namespace FD
    let our_mount_ns = namespace::MOUNT
        .open(unistd::getpid())
        .context("failed to open our own mount namespace")?;

    // Mount tmpfs at base_dir (for socket and mount points)
    // Note: base_dir was already created earlier before entering the namespace
    nix::mount::mount(
        Some("tmpfs"),
        base_dir.as_path(),
        Some("tmpfs"),
        nix::mount::MsFlags::empty(),
        None::<&str>,
    )
    .with_context(|| format!("failed to mount tmpfs at {}", base_dir.display()))?;

    // Enter container's mount namespace to capture trees with submounts
    let container_mount_namespace = namespace::MOUNT
        .open(options.process_status.global_pid)
        .context("could not access container mount namespace")?;
    container_mount_namespace
        .apply()
        .context("failed to enter container mount namespace")?;

    // Capture each container root entry with open_tree()
    let mut captured_trees = Vec::new();
    for entry in std::fs::read_dir(&container_root_path).with_context(|| {
        format!(
            "failed to read container root at {}",
            container_root_path.display()
        )
    })? {
        let entry = entry.context("failed to read directory entry")?;
        let file_name = entry.file_name();
        let file_name_str = file_name.to_string_lossy();

        // Skip special directories
        if file_name_str == "." || file_name_str == ".." {
            continue;
        }

        let source = entry.path();
        let source_cstr = CString::new(source.as_os_str().as_bytes())
            .with_context(|| format!("failed to create CString for {}", source.display()))?;

        // Check if the source is a directory using filesystem metadata
        let is_dir = entry.file_type().map(|ft| ft.is_dir()).unwrap_or_else(|e| {
            warn!(
                "Failed to get file type for {:?}, assuming non-directory: {}",
                source, e
            );
            false
        });

        // Capture this entry's tree (includes all submounts)
        match MountFd::open_tree_at(&source_cstr, OPEN_TREE_CLONE | AT_RECURSIVE) {
            Ok(tree_fd) => {
                captured_trees.push((file_name, tree_fd, is_dir));
            }
            Err(e) => {
                warn!("Failed to capture tree for {:?}: {}", source, e);
            }
        }
    }

    // Return to our own mount namespace (with tmpfs and idmapped host root)
    our_mount_ns
        .apply()
        .context("failed to return to our mount namespace")?;

    // Attach each captured tree to base_dir
    // Note: We DON'T apply idmap to container trees - idmap was applied to host root above
    for (file_name, tree_fd, is_dir) in captured_trees {
        let target = base_dir.join(&file_name);

        // Create mount point based on the actual file type
        if is_dir {
            if let Err(e) = std::fs::create_dir_all(&target) {
                warn!("Failed to create directory mount point {:?}: {}", target, e);
            }
        } else {
            // Ensure parent directory exists before creating file
            if let Some(parent) = target.parent() {
                if let Err(e) = std::fs::create_dir_all(parent) {
                    warn!("Failed to create parent directory for {:?}: {}", target, e);
                }
            }
            if let Err(e) = std::fs::File::create(&target) {
                warn!("Failed to create file mount point {:?}: {}", target, e);
            }
        }

        let target_cstr = CString::new(target.as_os_str().as_bytes())
            .with_context(|| format!("failed to create CString for {}", target.display()))?;

        if let Err(e) = tree_fd.attach_to(None, &target_cstr, 0) {
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
        ns.apply().context("failed to apply namespace")?;
    }

    // Step 7: Set UID/GID
    if supported_namespaces.contains(namespace::USER.name) {
        // Only try to set groups if not already denied
        if !setgroups_denied
            && let Err(e) = unistd::setgroups(&[])
            && !dropped_groups
        {
            Err(e).context("could not set groups")?;
        }
        unistd::setgid(options.gid).context("could not set group id")?;
        unistd::setuid(options.uid).context("could not set user id")?;
    }

    // Step 8: Drop capabilities
    capabilities::drop(
        options.process_status.effective_capabilities,
        options.process_status.last_cap,
    )
    .context("failed to apply capabilities")?;

    // Step 9: Setup PTY
    let pty_master = pty::open_ptm().context("failed to open pty master")?;
    pty::attach_pts(&pty_master).context("failed to setup pty slave")?;

    // Step 10: Send ready signal + PTY fd to parent
    let ready_msg = b"R";
    let pty_fd = pty_master.as_fd();
    options
        .socket
        .send(&[ready_msg], &[&pty_fd])
        .context("failed to send ready signal and pty fd to parent")?;

    // Step 11: Change to base_dir
    if let Err(e) = env::set_current_dir(&base_dir) {
        warn!(
            "failed to change directory to {}: {}",
            base_dir.display(),
            e
        );
    }

    // Step 12: Inherit LSM profile
    if let Some(profile) = lsm_profile {
        profile
            .inherit_profile()
            .context("failed to inherit lsm profile")?;
    }

    // Step 13: Execute the command
    // This will replace the current process (attach child) with the shell
    // When the shell exits, the parent will see it and exit accordingly
    // Use exec_in_overlay() since we're in the overlay environment with access
    // to both host binaries and container filesystem
    cmd.exec_in_overlay()
        .context("failed to execute command in overlay")?;

    // Should not reach here - exec replaces process
    unreachable!()
}
