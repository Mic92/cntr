use anyhow::Context;
use log::{info, warn};
use nix::sys::socket::{
    AddressFamily, Backlog, SockFlag, SockType, UnixAddr, accept, bind, listen, socket,
};
use std::fs;
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, FromRawFd, OwnedFd, RawFd};
use std::path::PathBuf;

use crate::daemon::protocol::{ExecRequest, ExecResponse};
use crate::procfs::ProcStatus;
use crate::result::Result;

pub(crate) const DAEMON_SOCKET_PATH: &str = "/var/lib/cntr/.exec.sock";

/// Get the fixed socket path
///
/// The socket is always at /var/lib/cntr/.exec.sock within the staging tmpfs.
/// Since the tmpfs is private to each container, there's no conflict between containers.
pub(crate) fn get_socket_path() -> PathBuf {
    PathBuf::from(DAEMON_SOCKET_PATH)
}

/// RAII wrapper for daemon socket that ensures cleanup on drop
///
/// This struct automatically removes the Unix domain socket file when it goes
/// out of scope, regardless of how the process exits (normal exit, signal, panic).
pub(crate) struct DaemonSocket {
    fd: OwnedFd,
    socket_path: PathBuf,
    process_status: ProcStatus,
}

impl DaemonSocket {
    /// Create and bind a new daemon socket
    ///
    /// The socket is created at /var/lib/cntr/.exec.sock within the staging tmpfs.
    /// This tmpfs must already be mounted at /var/lib/cntr before calling this function.
    ///
    /// # Arguments
    ///
    /// * `process_status` - Container process status (contains PID and capabilities)
    ///
    /// # Returns
    ///
    /// A new DaemonSocket that will be automatically cleaned up on drop
    pub(crate) fn bind(process_status: ProcStatus) -> Result<Self> {
        let socket_path = get_socket_path();
        Self::bind_internal(socket_path, process_status)
    }

    /// Create a DaemonSocket from a raw FD (received via SCM_RIGHTS)
    ///
    /// # Safety
    ///
    /// The caller must ensure the FD is a valid, listening Unix domain socket
    pub(crate) unsafe fn from_raw_fd(fd: RawFd, process_status: ProcStatus) -> Self {
        let socket_path = get_socket_path();

        DaemonSocket {
            fd: unsafe { OwnedFd::from_raw_fd(fd) },
            socket_path,
            process_status,
        }
    }

    /// Internal helper to create and bind the socket
    fn bind_internal(socket_path: PathBuf, process_status: ProcStatus) -> Result<Self> {
        // Create Unix domain socket
        let fd = socket(
            AddressFamily::Unix,
            SockType::Stream,
            SockFlag::SOCK_CLOEXEC,
            None,
        )
        .context("failed to create daemon socket")?;

        // Remove existing socket file if it exists (from previous run)
        let _ = fs::remove_file(&socket_path);

        // Bind to socket path
        let unix_addr = UnixAddr::new(&socket_path).with_context(|| {
            format!(
                "failed to create Unix address for {}",
                socket_path.display()
            )
        })?;

        bind(fd.as_raw_fd(), &unix_addr).with_context(|| {
            format!("failed to bind daemon socket to {}", socket_path.display())
        })?;

        // Listen for connections (backlog of 5)
        listen(&fd, Backlog::new(5).unwrap()).context("failed to listen on daemon socket")?;

        Ok(DaemonSocket {
            fd,
            socket_path,
            process_status,
        })
    }

    /// Try to accept and handle a single connection on the daemon socket
    ///
    /// This is non-blocking if the socket is set to non-blocking mode.
    ///
    /// # Returns
    ///
    /// - `Ok(true)` if a connection was handled
    /// - `Ok(false)` if no connection was available
    /// - `Err(...)` on error
    pub(crate) fn try_accept(&self) -> Result<bool> {
        match accept(self.fd.as_raw_fd()) {
            Ok(client_fd) => {
                let client_owned = unsafe { OwnedFd::from_raw_fd(client_fd) };

                // Handle the request in the same thread
                if let Err(e) = self.handle_request(&client_owned) {
                    warn!("Failed to handle exec request: {}", e);
                }

                Ok(true)
            }
            Err(nix::errno::Errno::EAGAIN) => {
                // No pending connections
                Ok(false)
            }
            Err(e) => {
                Err(e).context("failed to accept connection on daemon socket")?;
                Ok(false)
            }
        }
    }

    /// Handle a single exec request from a client
    ///
    /// This function:
    /// 1. Reads the ExecRequest from the client socket
    /// 2. Delegates to the executor to perform the exec
    /// 3. Sends back ExecResponse
    fn handle_request(&self, client_fd: &OwnedFd) -> Result<()> {
        // Read exec request from client
        let mut client_file = std::fs::File::from(client_fd.try_clone().unwrap());
        let request = ExecRequest::deserialize(&mut client_file)
            .context("failed to deserialize exec request")?;

        // Send acknowledgment that we're handling the request
        let response = ExecResponse::Ok;
        response
            .serialize(&mut client_file)
            .context("failed to send response to client")?;

        // Execute the command in the container
        crate::daemon::execute_in_container(&request, &self.process_status)
            .context("failed to execute command in container")?;

        Ok(())
    }
}

impl AsFd for DaemonSocket {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.fd.as_fd()
    }
}

impl AsRawFd for DaemonSocket {
    fn as_raw_fd(&self) -> RawFd {
        self.fd.as_raw_fd()
    }
}

impl Drop for DaemonSocket {
    fn drop(&mut self) {
        if self.socket_path.exists() {
            match fs::remove_file(&self.socket_path) {
                Ok(_) => {
                    info!("Removed daemon socket at {}", self.socket_path.display());
                }
                Err(e) => {
                    warn!(
                        "Failed to remove daemon socket at {}: {}",
                        self.socket_path.display(),
                        e
                    );
                }
            }
        }
    }
}
