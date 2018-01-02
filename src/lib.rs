extern crate libc;
extern crate cntr_nix;
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
use cntr_nix::pty::PtyMaster;
use cntr_nix::sys::signal::{self, Signal};
use cntr_nix::sys::socket::CmsgSpace;
use cntr_nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};
use cntr_nix::unistd::{self, ForkResult, Pid};
pub use container::{AVAILABLE_CONTAINER_TYPES, lookup_container_type};
use std::env;
use std::fs::File;
use std::os::unix::io::IntoRawFd;
use std::os::unix::prelude::*;
use std::process;
use types::{Error, Result};
pub use user_namespace::DEFAULT_ID_MAP;
use user_namespace::IdMap;
use void::Void;

#[macro_use]
pub mod types;
pub mod namespace;
mod cgroup;
mod user_namespace;
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
mod container;
pub mod fs;

pub struct Options {
    pub container_name: String,
    pub container_types: Vec<Box<container::Container>>,
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

    let supported_namespaces = tryfmt!(
        namespace::supported_namespaces(),
        "failed to list namespaces"
    );

    if !supported_namespaces.contains(namespace::MOUNT.name) {
        return errfmt!("the system has no support for mount namespaces");
    };

    let mount_namespace = tryfmt!(
        namespace::MOUNT.open(container_pid),
        "could not access mount namespace"
    );
    let mut other_namespaces = Vec::new();

    let other_kinds = &[
        namespace::UTS,
        namespace::CGROUP,
        namespace::PID,
        namespace::NET,
        namespace::IPC,
        namespace::USER,
    ];

    for kind in other_kinds {
        if !supported_namespaces.contains(kind.name) {
            continue;
        }
        other_namespaces.push(tryfmt!(
            kind.open(container_pid),
            "failed to open {} namespace",
            kind.name
        ));
    }

    tryfmt!(mount_namespace.apply(), "failed to apply mount namespace");
    tryfmt!(mountns::setup(&fs, mount_ready_sock, mount_namespace), "");
    for ns in other_namespaces {
        tryfmt!(ns.apply(), "failed to apply namespace");
    }

    if supported_namespaces.contains(namespace::USER.name) {
        tryfmt!(unistd::setgroups(&[]), "could not set groups");
        tryfmt!(
            unistd::setuid(unistd::Uid::from_raw(0)),
            "could not set user id"
        );
        tryfmt!(
            unistd::setgid(unistd::Gid::from_raw(0)),
            "could not set group id"
        );
    }

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

    if let Err(e) = env::set_current_dir("/var/lib/cntr") {
        warn!("failed to change directory to /var/lib/cntr: {}", e);
    }

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
            container::lookup_container_pid(opts.container_name.as_str(), &opts.container_types),
            ""
        );

    let (uid_map, gid_map) = tryfmt!(
        IdMap::new_from_pid(container_pid),
        "failed to read usernamespace properties of {}",
        container_pid
    );

    let cntrfs = tryfmt!(
        fs::CntrFs::new(&fs::CntrMountOptions {
            prefix: "/",
            splice_read: false,
            splice_write: false,
            uid_map: uid_map,
            gid_map: gid_map,
        }),
        "cannot mount filesystem"
    );
    let (parent_sock, child_sock) = tryfmt!(ipc::socket_pair(), "failed to set up ipc");

    match tryfmt!(unistd::fork(), "failed to fork") {
        ForkResult::Parent { child } => run_parent(child, &parent_sock, &cntrfs),
        ForkResult::Child => run_child(container_pid, &child_sock, cntrfs),
    }
}
