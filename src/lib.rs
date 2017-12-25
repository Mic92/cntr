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
extern crate void;

use cmd::Cmd;
use container::ContainerType;
use nix::pty::PtyMaster;
use nix::sys::signal::{self, Signal};
use nix::sys::socket::CmsgSpace;
use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};
use nix::unistd::{self, ForkResult, Pid};
use std::fs::File;
use std::os::unix::io::IntoRawFd;
use std::os::unix::prelude::*;
use std::process;
use types::{Error, Result};
use void::Void;

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
mod capabilities;
mod ipc;
pub mod container;
pub mod fs;

pub struct Options {
    pub container_name: String,
    pub container_type: Option<ContainerType>,
}

fn run_parent(pid: Pid, mount_ready_sock: &ipc::Socket, fs: &fs::CntrFs) -> Result<Void> {
    let ns = tryfmt!(
        mountns::MountNamespace::receive(mount_ready_sock),
        "failed to receive mount namespace from child"
    );

    let sessions = fs.spawn_sessions();

    let mut cmsgspace: CmsgSpace<[RawFd; 1]> = CmsgSpace::new();
    let (_, mut fds) = tryfmt!(
        mount_ready_sock.receive(1, &mut cmsgspace),
        "failed to receive pty file descriptor"
    );
    assert!(fds.len() == 1);
    let fd = fds.pop().unwrap();

    let master = unsafe { PtyMaster::from_raw_fd(fd.into_raw_fd()) };

    ns.cleanup();

    loop {
        tryfmt!(
            pty::forward(&master),
            "failed to forward terminal output of command"
        );
        match waitpid(pid, Some(WaitPidFlag::WUNTRACED)) {
            Ok(WaitStatus::Signaled(child, Signal::SIGSTOP, _)) => {
                let _ = signal::kill(unistd::getpid(), Signal::SIGSTOP);
                let _ = signal::kill(child, Signal::SIGCONT);
            }
            Ok(WaitStatus::Signaled(_, signal, _)) => {
                tryfmt!(
                    signal::kill(unistd::getpid(), signal),
                    "failed to send signal {:?} to our own process",
                    signal
                );
            }
            Ok(WaitStatus::Exited(_, status)) => {
                process::exit(status);
            }
            Ok(what) => {
                panic!("unexpected wait event happend {:?}", what);
            }
            Err(e) => {
                drop(sessions);
                return tryfmt!(Err(e), "waitpid failed");
            }
        };
    }
}

fn run_child(container_pid: Pid, mount_ready_sock: &ipc::Socket, fs: fs::CntrFs) -> Result<Void> {
    let target_caps = tryfmt!(
        capabilities::get(Some(container_pid)),
        "failed to get capabilities of target process"
    );

    tryfmt!(
        cgroup::move_to(unistd::getpid(), container_pid),
        "failed to change cgroup"
    );

    let cmd = tryfmt!(Cmd::new(container_pid), "");

    let kinds = tryfmt!(
        namespace::supported_namespaces(),
        "failed to list namespaces"
    );

    let mut container_mount_ns: Option<namespace::Namespace> = None;

    for kind in kinds {
        let ns = tryfmt!(kind.open(container_pid), "failed to open namespace");
        tryfmt!(ns.apply(), "failed to apply namespace");
        if ns.kind.name == namespace::MOUNT.name {
            container_mount_ns = Some(ns);
        }
    }

    if container_mount_ns.is_none() {
        return errfmt!("no mount namespace found for container");
    }

    tryfmt!(
        mountns::setup(&fs, mount_ready_sock, container_mount_ns.unwrap()),
        ""
    );

    tryfmt!(
        capabilities::set(Some(unistd::getpid()), &target_caps),
        "failed to apply capabilities"
    );

    let pty_master = tryfmt!(pty::open_ptm(), "open pty master");
    tryfmt!(pty::attach_pts(&pty_master), "failed to setup pty master");

    // we have to destroy f manually, since we only borrow fd here.
    let f = unsafe { File::from_raw_fd(pty_master.as_raw_fd()) };
    let res = mount_ready_sock.send(&[], &[&f]);
    f.into_raw_fd();
    tryfmt!(res, "failed to send pty file descriptor to parent process");

    let status = tryfmt!(cmd.run(), "");
    if let Some(signum) = status.signal() {
        let signal = tryfmt!(
            Signal::from_c_int(signum),
            "invalid signal received: {}",
            signum
        );
        tryfmt!(
            signal::kill(unistd::getpid(), signal),
            "failed to send signal {:?} to own pid",
            signal
        );
    }
    if let Some(code) = status.code() {
        process::exit(code);
    }
    panic!(
        "BUG! command exited successfully, \
        but was neither terminated by a signal nor has an exit code"
    );
}

pub fn run(opts: &Options) -> Result<Void> {
    let container_pid =
        tryfmt!(
            container::lookup_container_pid(&opts.container_name, opts.container_type.clone()),
            ""
        );

    let cntrfs = tryfmt!(
        fs::CntrFs::new(&fs::CntrMountOptions {
            prefix: "/",
            splice_read: false,
            splice_write: false,
        }),
        "cannot mount filesystem"
    );
    let (parent_sock, child_sock) = tryfmt!(ipc::socket_pair(), "failed to set up ipc");

    match tryfmt!(unistd::fork(), "failed to fork") {
        ForkResult::Parent { child } => run_parent(child, &parent_sock, &cntrfs),
        ForkResult::Child => run_child(container_pid, &child_sock, cntrfs),
    }
}
