pub(crate) use container_pid::lookup_container_type;

// Use a pure-Rust, mmap-based allocator (built on rustix) instead of libc
// malloc. This keeps heap allocations safe in children created via the raw
// fork syscall (rustix::runtime::kernel_fork), which bypasses libc's
// pthread_atfork handling.
#[global_allocator]
static GLOBAL_ALLOCATOR: rustix_dlmalloc::GlobalDlmalloc = rustix_dlmalloc::GlobalDlmalloc;

pub mod test_utils;

mod attach;
mod capabilities;
mod cgroup;
mod cmd;
mod container;
mod container_setup;
pub(crate) mod errors;
pub(crate) mod exec;
mod ipc;
mod lsm;
pub(crate) mod namespace;
mod passwd;
pub(crate) mod paths;
mod procfs;
mod pty;
pub mod syscalls;
pub(crate) use attach::{AttachOptions, attach};

pub mod cli;

/// AppArmor mode configuration
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApparmorMode {
    /// Automatically detect and apply AppArmor profile (default)
    Auto,
    /// Disable AppArmor profile application
    Off,
}
