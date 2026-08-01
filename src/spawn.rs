//! Minimal process spawning built on fork + execve.
//!
//! Replaces `std::process::Command` so that no libc spawn machinery
//! (posix_spawn, atfork handlers) is needed. Only supports the two cases
//! cntr has: capturing the output of a helper CLI (`run`) and replacing the
//! current process with the container command (`exec`).

use hashbrown::HashMap;
use rustix::event::{PollFd, PollFlags, poll};
use rustix::fs::{Access, Mode, OFlags, access, fcntl_setfl, open};
use rustix::io::Errno;
use rustix::pipe::pipe;
use rustix::process::{WaitOptions, WaitStatus, waitpid};
use rustix::runtime::{Fork, execve, exit_group, kernel_fork};
use rustix::stdio::{dup2_stderr, dup2_stdin, dup2_stdout};
use std::convert::Infallible;
use std::ffi::CString;
use std::os::fd::OwnedFd;
use typed_path::UnixPath;

use crate::container_pid::cmd::which;
use crate::env;

/// Captured result of a finished child process.
pub(crate) struct Output {
    status: WaitStatus,
    pub(crate) stdout: Vec<u8>,
    pub(crate) stderr: Vec<u8>,
}

impl Output {
    pub(crate) fn success(&self) -> bool {
        self.status.exit_status() == Some(0)
    }

    /// Human readable exit status for error messages.
    pub(crate) fn status(&self) -> String {
        match (self.status.exit_status(), self.status.terminating_signal()) {
            (Some(code), _) => format!("exit code {}", code),
            (_, Some(sig)) => format!("killed by signal {}", sig),
            _ => format!("{:?}", self.status),
        }
    }
}

fn cstring<S: AsRef<[u8]>>(s: S) -> Result<CString, Errno> {
    CString::new(s.as_ref()).map_err(|_| Errno::INVAL)
}

/// Argv/envp marshalled into CStrings, so that no allocation is needed
/// between fork and execve.
struct ExecArgs {
    path: CString,
    argv: Vec<CString>,
    envp: Vec<CString>,
}

impl ExecArgs {
    /// Replace the current process image. Only returns on error.
    fn exec(&self) -> Errno {
        let null_terminated = |strings: &[CString]| -> Vec<*const u8> {
            let mut ptrs: Vec<*const u8> = strings.iter().map(|s| s.as_ptr().cast()).collect();
            ptrs.push(std::ptr::null());
            ptrs
        };
        let argv = null_terminated(&self.argv);
        let envp = null_terminated(&self.envp);
        unsafe { execve(&self.path, argv.as_ptr(), envp.as_ptr()) }
    }
}

/// Look up `program` in the given PATH value unless it already contains a slash.
fn resolve_program(program: &str, path_var: Option<&Vec<u8>>) -> Result<CString, Errno> {
    // No let-chain here to stay compatible with Rust < 1.88 (Debian).
    if !program.contains('/') {
        if let Some(paths) = path_var.and_then(|p| std::str::from_utf8(p).ok()) {
            for dir in env::split_paths(paths) {
                let candidate = UnixPath::new(dir).join(program);
                if access(candidate.as_bytes(), Access::EXEC_OK).is_ok() {
                    return cstring(candidate.as_bytes());
                }
            }
        }
    }
    // Use as-is and let execve report ENOENT for unresolved names.
    cstring(program)
}

/// Replace the current process with `program`, using exactly the given
/// environment. The program is looked up in the PATH of `env`.
/// Only returns on error.
pub(crate) fn exec(
    program: &str,
    args: &[String],
    env: &HashMap<Vec<u8>, Vec<u8>>,
) -> Result<Infallible, Errno> {
    let mut argv = vec![cstring(program)?];
    for arg in args {
        argv.push(cstring(arg)?);
    }
    let mut envp = Vec::with_capacity(env.len());
    for (key, value) in env {
        let mut entry = key.clone();
        entry.push(b'=');
        entry.extend_from_slice(value);
        envp.push(cstring(entry)?);
    }
    let exec_args = ExecArgs {
        path: resolve_program(program, env.get(b"PATH".as_slice()))?,
        argv,
        envp,
    };
    Err(exec_args.exec())
}

/// Run `program args...` with the current environment, capturing stdout and
/// stderr. Stdin is redirected from /dev/null.
pub(crate) fn run(program: &str, args: &[&str]) -> Result<Output, Errno> {
    let Some(path) = which(program) else {
        return Err(Errno::NOENT);
    };
    let mut argv = vec![cstring(program)?];
    for arg in args {
        argv.push(cstring(arg)?);
    }
    let envp = env::vars()
        .iter()
        .map(|(key, value)| {
            let mut entry = key.clone();
            entry.push(b'=');
            entry.extend_from_slice(value);
            cstring(entry)
        })
        .collect::<Result<Vec<CString>, Errno>>()?;
    let exec_args = ExecArgs {
        path: cstring(path.as_bytes())?,
        argv,
        envp,
    };

    let (stdout_read, stdout_write) = pipe()?;
    let (stderr_read, stderr_write) = pipe()?;
    let devnull = open("/dev/null", OFlags::RDONLY, Mode::empty())?;

    // SAFETY: between fork and execve we only call async-signal-safe rustix
    // wrappers; all allocations were done above.
    match unsafe { kernel_fork() }? {
        Fork::Child(_) => {
            let ok = dup2_stdin(&devnull).is_ok()
                && dup2_stdout(&stdout_write).is_ok()
                && dup2_stderr(&stderr_write).is_ok();
            if ok {
                exec_args.exec();
            }
            exit_group(127);
        }
        Fork::ParentOf(child) => {
            drop(stdout_write);
            drop(stderr_write);

            let (stdout, stderr) = read_outputs(stdout_read, stderr_read)?;

            let status = loop {
                match waitpid(Some(child), WaitOptions::empty()) {
                    Ok(Some((_, status))) => break status,
                    Ok(None) | Err(Errno::INTR) => continue,
                    Err(e) => return Err(e),
                }
            };

            Ok(Output {
                status,
                stdout,
                stderr,
            })
        }
    }
}

/// Drain both pipes concurrently to avoid deadlocks when the child fills one
/// of them while we are blocked on the other.
fn read_outputs(stdout: OwnedFd, stderr: OwnedFd) -> Result<(Vec<u8>, Vec<u8>), Errno> {
    // Non-blocking reads let us drain both fds after each poll wakeup without
    // tracking which one is actually ready.
    fcntl_setfl(&stdout, OFlags::NONBLOCK)?;
    fcntl_setfl(&stderr, OFlags::NONBLOCK)?;
    let mut fds = [Some(stdout), Some(stderr)];
    let mut bufs = [Vec::new(), Vec::new()];
    let mut chunk = [0u8; 8192];

    while fds.iter().any(Option::is_some) {
        let mut poll_fds: Vec<PollFd> = fds
            .iter()
            .flatten()
            .map(|fd| PollFd::new(fd, PollFlags::IN))
            .collect();
        match poll(&mut poll_fds, None) {
            Ok(_) => {}
            Err(Errno::INTR) => continue,
            Err(e) => return Err(e),
        }
        drop(poll_fds);

        for (fd_slot, buf) in fds.iter_mut().zip(bufs.iter_mut()) {
            let Some(fd) = fd_slot else { continue };
            match rustix::io::read(&*fd, &mut chunk) {
                Ok(0) => *fd_slot = None,
                Ok(n) => buf.extend_from_slice(&chunk[..n]),
                Err(Errno::INTR) | Err(Errno::AGAIN) => {}
                Err(e) => return Err(e),
            }
        }
    }

    let [stdout_buf, stderr_buf] = bufs;
    Ok((stdout_buf, stderr_buf))
}

#[cfg(test)]
mod tests {
    use super::run;

    #[test]
    fn test_run_captures_output_and_status() {
        let out = run("sh", &["-c", "echo hello; echo err >&2; exit 3"]).unwrap();
        assert_eq!(out.stdout, b"hello\n");
        assert_eq!(out.stderr, b"err\n");
        assert!(!out.success());
        assert_eq!(out.status(), "exit code 3");

        let ok = run("sh", &["-c", "true"]).unwrap();
        assert!(ok.success());

        assert!(run("cntr-does-not-exist", &[]).is_err());
    }
}
