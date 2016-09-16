extern crate libc;
extern crate nix;
#[macro_use]
extern crate log;
extern crate core;
extern crate fuse;
extern crate time;
extern crate void;

use nix::unistd;
use pty::PtyFork;
use std::thread;
use types::{Error, Result};

#[macro_use]
pub mod types;
pub mod namespace;
mod cgroup;
mod pty;
mod logging;
mod cmd;
mod sigstr;
mod statvfs;
mod xattr;
pub mod fs;

pub struct Options {
    pub pid: libc::pid_t,
    pub mountpoint: String,
}

#[allow(unused_variables)]
fn run_parent(pty: PtyFork, opts: Options) -> Result<()> {
    if let PtyFork::Parent { .. } = pty {
        let child = thread::spawn(move || {
            if let PtyFork::Parent { ref pty_master, .. } = pty {
                pty::forward(pty_master)
            }
        });
        if let Err(_) = child.join() {
            return errfmt!("pty thread died");
        };
    }
    return Ok(());
}

fn run_child(opts: Options) -> Result<()> {
    let cntr = tryfmt!(fs::CntrFs::new("/"), "cannot mount filesystem");
    tryfmt!(cgroup::move_to(unistd::getpid(), opts.pid),
            "failed to change cgroup");
    let kinds = tryfmt!(namespace::supported_namespaces(),
                        "failed to list namespaces");
    for kind in kinds {
        let namespace = tryfmt!(kind.open(opts.pid), "failed to open namespace");
        tryfmt!(namespace.apply(), "failed to apply namespace");
    }
    fuse::mount(cntr, &opts.mountpoint, &[]);
    #[allow(unreachable_patterns)]
    let _ = tryfmt!(cmd::exec(opts.pid), "");
    Ok(())
}

pub fn run(opts: Options) -> Result<()> {
    tryfmt!(logging::init(), "failed to initialize logging");
    let res = tryfmt!(pty::fork(), "fork failed");
    if let PtyFork::Parent { .. } = res {
        run_parent(res, opts)
    } else {
        run_child(opts)
    }
}
