extern crate libc;
extern crate nix;
#[macro_use]
extern crate log;
extern crate core;
extern crate fuse;
extern crate time;
extern crate tempdir;
extern crate num_cpus;
extern crate chashmap;
extern crate parking_lot;

use nix::unistd;
use pty::PtyFork;
use tempdir::TempDir;
use types::{Error, Result};

#[macro_use]
pub mod types;
pub mod namespace;
mod cgroup;
mod ioctl;
mod pty;
mod logging;
mod cmd;
mod statvfs;
mod xattr;
mod fsuid;
pub mod fs;

pub struct Options {
    pub pid: unistd::Pid,
    pub mountpoint: String,
}

fn run_parent(pty: &PtyFork) -> Result<()> {
    if let PtyFork::Parent { ref pty_master, .. } = *pty {
        pty::forward(pty_master)
    }

    Ok(())
}

fn run_child(fs: fs::CntrFs, opts: &Options) -> Result<()> {
    tryfmt!(
        cgroup::move_to(unistd::getpid(), opts.pid),
        "failed to change cgroup"
    );
    let kinds = tryfmt!(
        namespace::supported_namespaces(),
        "failed to list namespaces"
    );
    for kind in kinds {
        let namespace = tryfmt!(kind.open(opts.pid), "failed to open namespace");
        tryfmt!(namespace.apply(), "failed to apply namespace");
    }

    let mountpoint = tryfmt!(
        TempDir::new("cntrfs"),
        "failed to create temporary mountpoint"
    );
    let _ = tryfmt!(fs.mount(mountpoint.path(), false), "mount()");

    println!("mount at {:?}", mountpoint.path());

    let result = cmd::run(opts.pid);
    let _ = nix::mount::umount(mountpoint.path());

    let _ = tryfmt!(result, "");
    Ok(())
}

pub fn run(opts: &Options) -> Result<()> {
    tryfmt!(logging::init(), "failed to initialize logging");
    let cntr_fs = tryfmt!(
        fs::CntrFs::new(opts.mountpoint.as_str(), false),
        "cannot mount filesystem"
    );

    let res = tryfmt!(pty::fork(), "fork failed");
    if let PtyFork::Parent { .. } = res {
        run_parent(&res)
    } else {
        run_child(cntr_fs, opts)
    }
}
