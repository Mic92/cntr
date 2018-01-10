use capabilities;
use cgroup;
use cmd::Cmd;
use fs;
use ipc;
use lsm;
use mountns;
use namespace;
use nix::sys::signal::{self, Signal};
use nix::unistd;
use nix::unistd::{Uid, Gid};
use procfs::ProcStatus;
use procfs::unix;
use pty;
use socket_proxy::{self, Listener};
use std::env;
use std::ffi::{CStr, OsStr};
use std::fs::File;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::prelude::*;
use std::path::PathBuf;
use std::process;
use tempdir::TempDir;
use types::{Error, Result};
use void::Void;

pub struct ChildOptions<'a> {
    pub command: Option<String>,
    pub arguments: Vec<String>,
    pub process_status: ProcStatus,
    pub mount_ready_sock: &'a ipc::Socket,
    pub fs: fs::CntrFs,
    pub home: Option<&'a CStr>,
    pub uid: Uid,
    pub gid: Gid,
}

fn send_fds(mount_ready_sock: &ipc::Socket, listeners: &Vec<Listener>) -> Result<()> {
    let pty_master = tryfmt!(pty::open_ptm(), "open pty master");
    tryfmt!(pty::attach_pts(&pty_master), "failed to setup pty master");
    let pty_file = unsafe { File::from_raw_fd(pty_master.into_raw_fd()) };

    let socket_num = listeners.len().to_string();
    tryfmt!(
        mount_ready_sock.send(&[socket_num.as_ref()], &[&pty_file]),
        "failed to send pty master"
    );

    for listener in listeners {
        let path: &OsStr = listener.address.as_ref();
        assert!(path.len() < 255);

        tryfmt!(
            mount_ready_sock.send(&[path.as_bytes()], &[&listener.socket]),
            "failed to send unix sockets to parent process"
        );

    }

    Ok(())
}

fn _run(cmd: Cmd) -> Result<Void> {
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
    eprintln!(
        "BUG! command exited successfully, \
        but was neither terminated by a signal nor has an exit code"
    );
    process::exit(1);
}

fn setup_nested_namespaces(
    options: &ChildOptions,
    mount_label: &Option<String>,
    sockets: Vec<PathBuf>,
) -> Result<(TempDir, Vec<Listener>)> {
    let supported_namespaces = tryfmt!(
        namespace::supported_namespaces(),
        "failed to list namespaces"
    );

    if !supported_namespaces.contains(namespace::MOUNT.name) {
        return errfmt!("the system has no support for mount namespaces");
    };

    let mount_namespace = tryfmt!(
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

        other_namespaces.push(tryfmt!(
            kind.open(options.process_status.global_pid),
            "failed to open {} namespace",
            kind.name
        ));
    }


    tryfmt!(mount_namespace.apply(), "failed to apply mount namespace");

    try!(
        mountns::setup(
            &options.fs,
            options.mount_ready_sock,
            mount_namespace,
            &mount_label,
        ),
    );

    let dropped_groups = if supported_namespaces.contains(namespace::USER.name) {
        unistd::setgroups(&[]).is_ok()
    } else {
        false
    };

    for ns in other_namespaces {
        tryfmt!(ns.apply(), "failed to apply namespace");
    }


    let (tempdir, listeners) = tryfmt!(
        socket_proxy::bind_paths(&sockets),
        "failed to setup socket proxy"
    );

    if supported_namespaces.contains(namespace::USER.name) {
        if let Err(e) = unistd::setgroups(&[]) {
            if !dropped_groups {
                tryfmt!(Err(e), "could not set groups");
            }
        }
        tryfmt!(unistd::setgid(options.gid), "could not set group id");
        tryfmt!(unistd::setuid(options.uid), "could not set user id");
    }

    Ok((tempdir, listeners))
}

pub fn run(options: &ChildOptions) -> Result<Void> {
    let lsm_profile = tryfmt!(
        lsm::read_profile(options.process_status.global_pid),
        "failed to get lsm profile"
    );

    let mount_label = if let Some(ref p) = lsm_profile {
        tryfmt!(
            p.mount_label(options.process_status.global_pid),
            "failed to read mount options"
        )
    } else {
        None
    };

    tryfmt!(
        cgroup::move_to(unistd::getpid(), options.process_status.global_pid),
        "failed to change cgroup"
    );

    let cmd = tryfmt!(
        Cmd::new(
            options.command.clone(),
            options.arguments.clone(),
            options.process_status.global_pid,
            options.home,
        ),
        ""
    );

    let sockets = tryfmt!(
        unix::read_open_sockets(),
        "failed to get current open unix sockets"
    );

    let (socket_dir, listeners) = try!(setup_nested_namespaces(options, &mount_label, sockets));

    try!(send_fds(options.mount_ready_sock, &listeners));

    tryfmt!(
        capabilities::drop(options.process_status.effective_capabilities),
        "failed to apply capabilities"
    );

    if let Err(e) = env::set_current_dir("/var/lib/cntr") {
        warn!("failed to change directory to /var/lib/cntr: {}", e);
    }

    if let Some(profile) = lsm_profile {
        tryfmt!(profile.inherit_profile(), "failed to inherit lsm profile");
    }

    _run(cmd)
}
