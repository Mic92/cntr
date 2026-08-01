use alloc::string::String;
use rustix::io::Errno;
use thiserror::Error;
use typed_path::UnixPathBuf;

use crate::container_pid::RawPid;

/// Errors that can occur while resolving a container name to a PID.
#[derive(Debug, Error)]
pub enum Error {
    /// The container runtime CLI needed for this backend is not installed.
    #[error("{runtime} runtime not found: '{tool}' command is not available")]
    RuntimeNotFound {
        runtime: &'static str,
        tool: &'static str,
    },
    /// Spawning the runtime CLI failed.
    #[error("failed to execute command: {command}")]
    CommandFailedToRun {
        command: String,
        #[source]
        source: Errno,
    },
    /// The runtime CLI ran but exited with an error.
    #[error("{command} failed (exit status {status}): {stderr}")]
    CommandFailed {
        command: String,
        status: String,
        stderr: String,
    },
    /// The runtime CLI produced output we could not parse.
    #[error("unexpected output from {command}: {message}")]
    UnexpectedOutput { command: String, message: String },
    /// The runtime reported a PID that is not a number.
    #[error("invalid PID '{pid}' reported by {runtime} for container '{container}'")]
    InvalidPid {
        pid: String,
        runtime: &'static str,
        container: String,
        #[source]
        source: core::num::ParseIntError,
    },
    /// The container exists but is not running.
    #[error("container '{0}' is not running")]
    NotRunning(String),
    /// No container matched the given name/ID for this backend.
    #[error("container '{container}' not found: {message}")]
    ContainerNotFound { container: String, message: String },
    /// Reading a file or directory failed.
    #[error("failed to read {}", path.display())]
    Io {
        path: UnixPathBuf,
        #[source]
        source: Errno,
    },
    /// The given container ID is not a valid process ID.
    #[error("'{0}' is not a valid PID (process ID)")]
    InvalidProcessId(String, #[source] core::num::ParseIntError),
    /// No process with the given PID exists.
    #[error("no process with PID {0} found")]
    NoSuchProcess(RawPid),

    /// None of the tried container runtimes could resolve the container.
    #[error("failed to find container '{container}' - tried the following runtimes:{tried}")]
    NoRuntimeMatched { container: String, tried: String },
}
