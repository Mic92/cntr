use log::warn;
use nix::sys::signal::{self, Signal};
use nix::unistd;
use nix::unistd::{Gid, Uid};
use simple_error::{bail, try_with};
use std::env;
use std::fs::File;
use std::os::unix::io::IntoRawFd;
use std::os::unix::prelude::*;
use std::path::PathBuf;
use std::process;

use crate::capabilities;
use crate::cgroup;
use crate::cmd::Cmd;
use crate::fs;
use crate::ipc;
use crate::lsm;
use crate::mountns;
use crate::namespace;
use crate::procfs::ProcStatus;
use crate::pty;
use crate::result::Result;

pub struct ChildOptions<'a> {
    pub command: Option<String>,
    pub arguments: Vec<String>,
    pub process_status: ProcStatus,
    pub mount_ready_sock: &'a ipc::Socket,
    pub fs: fs::CntrFs,
    pub home: Option<PathBuf>,
    pub uid: Uid,
    pub gid: Gid,
}

pub fn run(options: &ChildOptions) -> Result<()> {
    let lsm_profile = try_with!(
        lsm::read_profile(options.process_status.global_pid),
        "failed to get lsm profile"
    );

    let mount_label = if let Some(ref p) = lsm_profile {
        try_with!(
            p.mount_label(options.process_status.global_pid),
            "failed to read mount options"
        )
    } else {
        None
    };

    try_with!(
        cgroup::move_to(unistd::getpid(), options.process_status.global_pid),
        "failed to change cgroup"
    );

    let cmd = Cmd::new(
        options.command.clone(),
        options.arguments.clone(),
        options.process_status.global_pid,
        options.home.clone(),
    )?;

    let supported_namespaces = try_with!(
        namespace::supported_namespaces(),
        "failed to list namespaces"
    );

    if !supported_namespaces.contains(namespace::MOUNT.name) {
        bail!("the system has no support for mount namespaces")
    };

    let mount_namespace = try_with!(
        namespace::MOUNT.open(options.process_status.global_pid),
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
        if kind.is_same(options.process_status.global_pid) {
            continue;
        }

        other_namespaces.push(try_with!(
            kind.open(options.process_status.global_pid),
            "failed to open {} namespace",
            kind.name
        ));
    }

    try_with!(mount_namespace.apply(), "failed to apply mount namespace");

    mountns::setup(
        &options.fs,
        options.mount_ready_sock,
        mount_namespace,
        &mount_label,
    )?;
    let dropped_groups = if supported_namespaces.contains(namespace::USER.name) {
        unistd::setgroups(&[]).is_ok()
    } else {
        false
    };

    for ns in other_namespaces {
        try_with!(ns.apply(), "failed to apply namespace");
    }

    if supported_namespaces.contains(namespace::USER.name) {
        if let Err(e) = unistd::setgroups(&[]) {
            if !dropped_groups {
                try_with!(Err(e), "could not set groups");
            }
        }
        try_with!(unistd::setgid(options.gid), "could not set group id");
        try_with!(unistd::setuid(options.uid), "could not set user id");
    }

    try_with!(
        capabilities::drop(options.process_status.effective_capabilities),
        "failed to apply capabilities"
    );

    let pty_master = try_with!(pty::open_ptm(), "open pty master");
    try_with!(pty::attach_pts(&pty_master), "failed to setup pty master");

    // we have to destroy f manually, since we only borrow fd here.
    let f = unsafe { File::from_raw_fd(pty_master.as_raw_fd()) };
    let res = options.mount_ready_sock.send(&[], &[&f]);
    let _ = f.into_raw_fd();
    try_with!(res, "failed to send pty file descriptor to parent process");

    if let Err(e) = env::set_current_dir("/var/lib/cntr") {
        warn!("failed to change directory to /var/lib/cntr: {}", e);
    }

    if let Some(profile) = lsm_profile {
        try_with!(profile.inherit_profile(), "failed to inherit lsm profile");
    }

    let status = cmd.run()?;
    if let Some(signum) = status.signal() {
        let signal = try_with!(
            Signal::try_from(signum),
            "invalid signal received: {}",
            signum
        );
        try_with!(
            signal::kill(unistd::getpid(), signal),
            "failed to send signal {:?} to own pid",
            signal
        );
    }
    if let Some(code) = status.code() {
        process::exit(code);
    }
    eprintln!(
        "BUG! command exited successfully, \
         but was neither terminated by a signal nor has an exit code"
    );
    process::exit(1);
}
