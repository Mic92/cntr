use anyhow::{Context, bail};
use log::{debug, warn};
use nix::fcntl::AtFlags;
use nix::sys::stat::{SFlag, fstatat};
use nix::unistd;
use std::env;
use std::ffi::CString;
use std::os::unix::io::{AsFd, BorrowedFd, OwnedFd, RawFd};
use std::os::unix::prelude::*;
use std::path::{Path, PathBuf};

use crate::cgroup;
use crate::cmd::Cmd;
use crate::ipc;
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
        let tree = match MountFd::open_tree_at(None, &mount_cstr, OPEN_TREE_CLONE | AT_RECURSIVE) {
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

/// Capture and attach container filesystem trees
///
/// This function:
/// 1. Enters the container's mount namespace
/// 2. Captures each root entry using open_tree() (preserves submounts)
/// 3. Returns to our mount namespace
/// 4. Attaches the captured trees to base_dir
fn capture_and_attach_container_trees(
    container_root_fd: std::fs::File,
    container_pid: unistd::Pid,
    our_mount_ns: namespace::Namespace,
    base_dir: &Path,
) -> Result<()> {
    // Enter container's mount namespace to capture trees with submounts
    namespace::MOUNT
        .open(container_pid)
        .context("could not access container mount namespace")?
        .apply()
        .context("failed to enter container mount namespace")?;

    // Open container root directory
    let mut dir = nix::dir::Dir::from_fd(OwnedFd::from(container_root_fd))
        .context("failed to create Dir from container root FD")?;

    // Collect entries first to avoid borrow conflicts when using dir.as_fd() later
    let entries: Vec<_> = dir
        .iter()
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            let name = entry.file_name().to_bytes();
            name != b"." && name != b".."
        })
        .filter_map(|entry| {
            let name = CString::new(entry.file_name().to_bytes()).ok()?;
            Some((name, entry.file_type()))
        })
        .collect();

    let dir_fd = dir.as_fd();

    // Capture each entry as a mount tree
    let captured_trees: Vec<_> = entries
        .iter()
        .filter_map(|(name, file_type)| {
            // Determine if entry is a directory
            let is_dir = match file_type {
                Some(nix::dir::Type::Directory) => true,
                Some(_) => false,
                None => fstatat(dir_fd, name.as_c_str(), AtFlags::AT_SYMLINK_NOFOLLOW)
                    .map(|stat| SFlag::from_bits_truncate(stat.st_mode).contains(SFlag::S_IFDIR))
                    .unwrap_or_else(|e| {
                        warn!("Failed to stat {:?}, assuming non-directory: {}", name, e);
                        false
                    }),
            };

            // Capture tree
            match MountFd::open_tree_at(
                Some(dir_fd),
                name.as_c_str(),
                OPEN_TREE_CLONE | AT_RECURSIVE,
            ) {
                Ok(tree) => {
                    let name_os = std::ffi::OsStr::from_bytes(name.as_bytes()).to_owned();
                    Some((name_os, tree, is_dir))
                }
                Err(e) => {
                    warn!("Failed to capture tree for {:?}: {}", name, e);
                    None
                }
            }
        })
        .collect();

    // Return to our mount namespace
    our_mount_ns
        .apply()
        .context("failed to return to our mount namespace")?;

    // Attach captured trees to base_dir
    for (name, tree, is_dir) in captured_trees {
        let target = base_dir.join(&name);

        // Create mount point
        let mount_point_created = if is_dir {
            std::fs::create_dir_all(&target).is_ok()
        } else {
            target
                .parent()
                .map(|p| std::fs::create_dir_all(p).is_ok())
                .unwrap_or(true)
                && std::fs::File::create(&target).is_ok()
        };

        if !mount_point_created {
            warn!("Failed to create mount point {:?}", target);
            continue;
        }

        let target_cstr = CString::new(target.as_os_str().as_bytes())
            .with_context(|| format!("failed to create CString for {}", target.display()))?;

        if let Err(e) = tree.attach_to(None, &target_cstr, 0) {
            warn!("Failed to attach tree to {:?}: {}", target, e);
        }
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
///    - Open container's root via /proc/<pid>/root as FD (handles chroot)
///    - Create private mount namespace
///    - Create tmpfs at {base_dir}
///    - Enter container's mount namespace
///    - Capture each container entry with open_tree() using the FD (includes submounts)
///    - Return to parent namespace
///    - Attach captured trees to {base_dir}/*
/// 5. Enter other container namespaces (USER, NET, PID, IPC, UTS, CGROUP)
/// 6. Set UID/GID and drop capabilities
/// 7. Create daemon socket and setup PTY
/// 8. Execute the command
///
/// This function never returns on success - it replaces the current process.
pub(crate) fn run(options: &mut ChildOptions) -> Result<std::convert::Infallible> {
    // Step 1: Move to container's cgroup
    cgroup::move_to(unistd::getpid(), options.process_status.global_pid)
        .context("failed to change cgroup")?;

    // Step 3: Prepare command to execute
    let cmd = Cmd::new(
        options.command.clone(),
        options.arguments.clone(),
        options.process_status.global_pid,
        options.effective_home.clone(),
    )
    .with_context(|| {
        format!(
            "failed to prepare command for container PID {}",
            options.process_status.global_pid
        )
    })?;

    // Step 4: Open other namespaces (not mount - we handle that specially)
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

    // Open container's root as a file descriptor (handles chroot containers)
    // This FD will remain valid even after entering the container's mount namespace,
    // allowing us to access the container's root even if /proc is not mounted inside
    let proc_root_path = format!("/proc/{}/root", options.process_status.global_pid);
    let container_root_fd = std::fs::File::open(&proc_root_path)
        .with_context(|| format!("failed to open container root at {}", proc_root_path))?;

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

    // Capture container filesystem and attach to base_dir
    capture_and_attach_container_trees(
        container_root_fd,
        options.process_status.global_pid,
        our_mount_ns,
        &base_dir,
    )
    .context("failed to capture and attach container trees")?;

    // Step 6: Enter other container namespaces and apply security context
    let in_user_ns = other_namespaces.iter().any(|ns| {
        // Check if any namespace in the collection is a USER namespace
        ns.kind.name == namespace::USER.name
    });

    for ns in other_namespaces {
        ns.apply().context("failed to apply namespace")?;
    }

    // Step 7: Setup PTY (before applying AppArmor profile)
    let pty_master = pty::open_ptm().context("failed to open pty master")?;
    pty::attach_pts(&pty_master).context("failed to setup pty slave")?;

    // Step 8: Apply security context (UID/GID, capabilities) - NOT AppArmor yet
    crate::container_setup::apply_security_context(&mut options.process_status, in_user_ns)?;

    // Step 9: Send ready signal + PTY fd to parent
    let ready_msg = b"R";
    let pty_fd = pty_master.as_fd();
    options
        .socket
        .send(&[ready_msg], &[&pty_fd])
        .context("failed to send ready signal and pty fd to parent")?;

    // Step 10: Change to base_dir
    if let Err(e) = env::set_current_dir(&base_dir) {
        warn!(
            "failed to change directory to {}: {}",
            base_dir.display(),
            e
        );
    }

    // Step 11: Apply AppArmor profile just before exec
    if let Some(profile) = &mut options.process_status.lsm_profile {
        profile
            .inherit_profile()
            .context("failed to inherit AppArmor profile")?;
    }

    // Step 12: Execute the command
    // This will replace the current process (attach child) with the shell
    // When the shell exits, the parent will see it and exit accordingly
    // Use exec_in_overlay() since we're in the overlay environment with access
    // to both host binaries and container filesystem
    cmd.exec_in_overlay()
        .context("failed to execute command in overlay")
}
