// SPDX-License-Identifier: MIT
//! fork()/_exit() wrappers based on libc

use rustix::io::Errno;
use rustix::process::Pid;

/// Result of a successful [`fork`].
pub(crate) enum Fork {
    /// This is the child process.
    Child,
    /// This is the parent process; the field is the child's PID.
    ParentOf(Pid),
}

/// Create a child process.
pub(crate) fn fork() -> Result<Fork, Errno> {
    // SAFETY: fork() has no memory-safety preconditions; cntr is
    // single-threaded, so the child can keep using the heap and std.
    match unsafe { libc::fork() } {
        -1 => Err(Errno::from_raw_os_error(
            std::io::Error::last_os_error().raw_os_error().unwrap_or(0),
        )),
        0 => Ok(Fork::Child),
        pid => Ok(Fork::ParentOf(Pid::from_raw(pid).expect("non-zero pid"))),
    }
}

/// Terminate the process immediately without running libc/std cleanup.
pub(crate) fn exit(status: i32) -> ! {
    // SAFETY: _exit() only takes an exit status and does not return.
    unsafe { libc::_exit(status) }
}
