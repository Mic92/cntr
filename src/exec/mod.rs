use anyhow::{Context, bail};
use nix::sys::socket::{self, AddressFamily, SockFlag, SockType, UnixAddr, connect};
use nix::sys::wait::{WaitStatus, waitpid};
use nix::unistd::{self, ForkResult};
use std::io::{self, ErrorKind};
use std::os::fd::{AsRawFd, IntoRawFd};
use std::process;

use crate::cmd::Cmd;
use crate::container::ContainerContext;
use crate::container_setup;
use crate::daemon;
use crate::daemon::protocol::{ExecRequest, ExecResponse};
use crate::pty;
use crate::result::Result;
use crate::syscalls::capability;

/// Execute a command in a container via the daemon socket (T033)
///
/// Daemon mode: Must be run from inside 'cntr attach' session.
/// Connects to daemon socket at /var/lib/cntr/.exec.sock
///
/// Arguments:
/// - exe: Optional command to execute (None = default shell)
/// - args: Arguments to pass to the command
pub fn exec_daemon(exe: Option<String>, args: Vec<String>) -> Result<()> {
    // Get daemon socket path (fixed location)
    let socket_path = daemon::get_socket_path();

    if !socket_path.exists() {
        bail!(
            "Daemon socket not found at {}. \
            Are you running this from inside 'cntr attach'? \
            The attach process must be running to use 'cntr exec' in daemon mode.\n\
            \n\
            Hint: To exec directly into a container, use: cntr exec -t TYPE CONTAINER_ID -- COMMAND",
            socket_path.display()
        );
    }

    // Create Unix domain socket for client
    let client_sock = socket::socket(
        AddressFamily::Unix,
        SockType::Stream,
        SockFlag::SOCK_CLOEXEC,
        None,
    )
    .context("failed to create client socket")?;

    // Connect to daemon socket
    let unix_addr = UnixAddr::new(&socket_path).with_context(|| {
        format!(
            "failed to create Unix address for {}",
            socket_path.display()
        )
    })?;

    if let Err(e) = connect(client_sock.as_raw_fd(), &unix_addr) {
        match e {
            nix::errno::Errno::ECONNREFUSED => {
                bail!(
                    "Connection refused to daemon socket at {}. \
                    The socket file exists but no daemon is listening. \
                    The 'cntr attach' process may have died unexpectedly.",
                    socket_path.display()
                );
            }
            nix::errno::Errno::ENOENT => {
                bail!(
                    "Daemon socket disappeared at {}. \
                    The 'cntr attach' process terminated while we were connecting.",
                    socket_path.display()
                );
            }
            _ => {
                Err(e).with_context(|| {
                    format!(
                        "failed to connect to daemon socket at {}",
                        socket_path.display()
                    )
                })?;
            }
        }
    }

    // Create exec request
    let request = ExecRequest::new(exe.clone(), args.clone());

    // Send request to daemon
    let mut client_file = std::fs::File::from(client_sock);
    if let Err(e) = request.serialize(&mut client_file) {
        // Check if the error is due to daemon death (broken pipe)
        if let Some(io_err) = e.source().and_then(|s| s.downcast_ref::<io::Error>()) {
            match io_err.kind() {
                ErrorKind::BrokenPipe | ErrorKind::ConnectionReset => {
                    bail!(
                        "Daemon closed connection while sending request. \
                        The 'cntr attach' process died unexpectedly."
                    );
                }
                _ => {}
            }
        }
        return Err(e);
    }

    // Wait for response from daemon
    let response = match ExecResponse::deserialize(&mut client_file) {
        Ok(resp) => resp,
        Err(e) => {
            // Check if the error is due to daemon death (EOF or connection issues)
            if let Some(io_err) = e.source().and_then(|s| s.downcast_ref::<io::Error>()) {
                match io_err.kind() {
                    ErrorKind::UnexpectedEof => {
                        bail!(
                            "Daemon closed connection before sending response. \
                            The 'cntr attach' process died while processing the exec request."
                        );
                    }
                    ErrorKind::BrokenPipe | ErrorKind::ConnectionReset => {
                        bail!(
                            "Lost connection to daemon while waiting for response. \
                            The 'cntr attach' process died unexpectedly."
                        );
                    }
                    _ => {}
                }
            }
            Err(e).context("failed to receive response from daemon")?;
            unreachable!()
        }
    };

    // Check response
    match response {
        ExecResponse::Ok => {
            // Daemon acknowledged the request and will handle the exec
            // The client process exits here - the daemon handles the actual command execution
            Ok(())
        }
        ExecResponse::Error(msg) => {
            bail!("Daemon rejected exec request: {}", msg);
        }
    }
}

/// Execute a command directly in a container (T034)
///
/// Direct mode: Directly access container by ID/name with PTY.
/// This provides similar functionality to attach but without the mount overlay or daemon.
///
/// Arguments:
/// - container_name: Container ID, name, or PID
/// - container_types: List of container types to try
/// - exe: Optional command to execute (None = default shell)
/// - args: Arguments to pass to the command
pub fn exec_direct(
    container_name: &str,
    container_types: &[Box<dyn container_pid::Container>],
    exe: Option<String>,
    args: Vec<String>,
) -> Result<()> {
    // Verify mount API capability
    if !capability::has_mount_api() {
        bail!(
            "Linux mount API is not available. cntr requires kernel 6.8+ with mount API support.\n\
             Please upgrade your kernel or use an older version of cntr with FUSE support."
        );
    }

    // Lookup container and get its context
    let ctx = ContainerContext::lookup(container_name, container_types)?;

    // Create PTY for interactive command execution
    let pty_master = pty::open_ptm().context("failed to open pty master")?;

    // Fork: child enters container and execs, parent forwards PTY I/O
    let res = unsafe { unistd::fork() };
    match res.context("failed to fork")? {
        ForkResult::Parent { child } => {
            // Parent: Forward PTY I/O and wait for child
            exec_direct_parent(child, &pty_master)
        }
        ForkResult::Child => {
            // Child: Setup PTY slave, enter container, exec command
            if let Err(e) = exec_direct_child(&ctx, exe, args, &pty_master) {
                eprintln!("exec_direct child failed: {}", e);
                process::exit(1);
            }
            // Should not reach here - exec_direct_child calls process::exit
            unreachable!()
        }
    }
}

/// Parent process for exec_direct: Forward PTY and wait for child
fn exec_direct_parent(child_pid: nix::unistd::Pid, pty_master: &nix::pty::PtyMaster) -> Result<()> {
    // Close master PTY fd in child before forwarding
    // (child has slave end)
    let pty_file = unsafe {
        use std::fs::File;
        use std::os::fd::FromRawFd;
        File::from_raw_fd(pty_master.as_raw_fd())
    };

    // Forward PTY I/O
    // This will block until child exits or PTY closes
    let _ = pty::forward(&pty_file);

    // Don't close the PTY file (avoid double-free)
    let _ = pty_file.into_raw_fd();

    // Wait for child to exit
    match waitpid(child_pid, None) {
        Ok(WaitStatus::Exited(_, status)) => {
            process::exit(status);
        }
        Ok(WaitStatus::Signaled(_, signal, _)) => {
            // Child was signaled - send same signal to ourselves
            nix::sys::signal::kill(unistd::getpid(), signal)
                .with_context(|| format!("failed to send signal {:?} to own process", signal))?;
        }
        Ok(status) => {
            bail!("child exited with unexpected status: {:?}", status);
        }
        Err(e) => {
            Err(e).context("failed to wait for child")?;
        }
    }

    Ok(())
}

/// Child process for exec_direct: Enter container and exec command
fn exec_direct_child(
    ctx: &ContainerContext,
    exe: Option<String>,
    args: Vec<String>,
    pty_master: &nix::pty::PtyMaster,
) -> Result<()> {
    // Attach PTY slave
    pty::attach_pts(pty_master).context("failed to setup pty slave")?;

    // Prepare command to execute
    let cmd = Cmd::new(exe, args, ctx.process_status.global_pid, None)?;

    // Enter container: cgroup, namespaces, security context (LSM, UID/GID, capabilities)
    container_setup::enter_container(ctx.process_status.global_pid, &ctx.process_status)?;

    // Resolve container's root path (handles chroot containers)
    let proc_root_path = format!("/proc/{}/root", ctx.process_status.global_pid);
    let container_root = std::fs::read_link(&proc_root_path)
        .with_context(|| format!("failed to read container root from {}", proc_root_path))?;

    // Chroot to container's root
    nix::unistd::chroot(&container_root)
        .with_context(|| format!("failed to chroot to {}", container_root.display()))?;
    std::env::set_current_dir("/").context("failed to chdir to / after chroot")?;

    // Execute the command (replaces current process)
    // Now we're in the container's root, so paths work correctly
    let status = cmd.run()?;

    // Handle exit status (if run() somehow returns)
    if let Some(code) = status.code() {
        process::exit(code);
    }

    Ok(())
}
