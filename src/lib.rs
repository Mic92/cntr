pub(crate) use container_pid::lookup_container_type;

pub mod test_utils;

mod attach;
mod capabilities;
mod cgroup;
mod cmd;
mod container;
mod container_setup;
pub(crate) mod exec;
mod ipc;
mod lsm;
pub(crate) mod namespace;
pub(crate) mod paths;
mod procfs;
mod pty;
mod result;
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
