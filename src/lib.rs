#![cfg_attr(not(test), no_std)]

extern crate alloc;

#[cfg(test)]
extern crate std;

pub(crate) mod container_pid;
pub(crate) use container_pid::lookup_container_type;

mod attach;
mod capabilities;
mod cgroup;
mod cmd;
mod container;
mod container_setup;
mod env;
pub(crate) mod errors;
pub(crate) mod exec;
mod fsutil;
mod ipc;
pub mod logging;
mod lsm;
pub(crate) mod namespace;
mod passwd;
pub(crate) mod paths;
mod procfs;
mod pty;
mod spawn;
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
