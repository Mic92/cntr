extern crate libc;
extern crate nix;
#[macro_use]
extern crate log;
extern crate core;
extern crate fuse;
extern crate time;
extern crate tempdir;
extern crate num_cpus;
extern crate concurrent_hashmap;
extern crate parking_lot;

use cmd::Cmd;
use nix::unistd;
use pty::PtyFork;
use std::fs::File;
use std::io::Read;
use std::os::unix::prelude::*;
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
mod fusefd;
mod files;
mod mountns;
mod seccomp;
pub mod fs;

pub struct Options {
    pub pid: unistd::Pid,
    pub mountpoint: String,
}

fn run_parent(mut mount_ready_file: File, fs: fs::CntrFs, pty: &PtyFork) -> Result<()> {
    let mut buf = [0 as u8; 1];
    tryfmt!(
        mount_ready_file.read_exact(&mut buf),
        "child process failed to mount fuse"
    );

    let sessions = fs.spawn_sessions();

    if let PtyFork::Parent { ref pty_master, .. } = *pty {
        pty::forward(pty_master)
    }

    drop(sessions);

    Ok(())
}

fn run_child(mount_ready_file: File, fs: fs::CntrFs, opts: &Options) -> Result<()> {
    tryfmt!(
        cgroup::move_to(unistd::getpid(), opts.pid),
        "failed to change cgroup"
    );

    let cmd = tryfmt!(Cmd::new(opts.pid), "");

    let kinds = tryfmt!(
        namespace::supported_namespaces(),
        "failed to list namespaces"
    );

    let mut container_mount_ns: Option<namespace::Namespace> = None;

    for kind in kinds {
        let ns = tryfmt!(kind.open(opts.pid), "failed to open namespace");
        tryfmt!(ns.apply(), "failed to apply namespace");
        if ns.kind.name == namespace::MOUNT.name {
            container_mount_ns = Some(ns);
        }
    }

    if container_mount_ns.is_none() {
        return errfmt!("no mount namespace found for container");
    }

    let ns = tryfmt!(
        mountns::setup(fs, mount_ready_file, container_mount_ns.unwrap()),
        ""
    );

    let result = cmd.run();

    let _ = tryfmt!(result, "");

    // delay destruction of mountns and associated mountpoints after command has exited
    drop(ns);

    Ok(())
}

pub fn run(opts: &Options) -> Result<()> {
    let cntrfs = tryfmt!(
        fs::CntrFs::new(&fs::CntrMountOptions {
            prefix: opts.mountpoint.as_str(),
            splice_read: false,
            splice_write: false,
        }),
        "cannot mount filesystem"
    );
    let (parent_fd, child_fd) = tryfmt!(nix::unistd::pipe(), "failed to create pipe");
    let parent_file = unsafe { File::from_raw_fd(parent_fd) };
    let child_file = unsafe { File::from_raw_fd(child_fd) };

    let res = tryfmt!(pty::fork(), "fork failed");
    if let PtyFork::Parent { .. } = res {
        run_parent(parent_file, cntrfs, &res)
    } else {
        tryfmt!(logging::init(), "failed to initialize logging");
        run_child(child_file, cntrfs, opts)
    }
}
