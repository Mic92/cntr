extern crate libc;
extern crate nix;
#[macro_use]
extern crate log;
extern crate core;
extern crate fuse;
extern crate time;
extern crate void;
extern crate thread_scoped;
extern crate tempdir;

use nix::sys::socket::{socketpair, AddressFamily, SockType, SockFlag, sendmsg, MsgFlags,
                       ControlMessage, CmsgSpace, recvmsg};
use nix::sys::uio::IoVec;
use nix::unistd;
use pty::PtyFork;
use std::os::unix::io::RawFd;
use thread_scoped::scoped;
use types::{Error, Result};
use fuse::Session;
use tempdir::TempDir;

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
    pub pid: unistd::Pid,
    pub mountpoint: String,
}

#[allow(unused_variables)]
fn run_parent(fs: fs::CntrFs, socket: RawFd, pty: PtyFork, opts: Options) -> Result<()> {
    let fuse_fd = tryfmt!(receive_fd(socket), "failed to receive fuse handle");

    let guard = unsafe {
        scoped(move || {
            Session::new_from_fd(fs, fuse_fd).run()
        })
    };

    if let PtyFork::Parent { ref pty_master, .. } = pty {
        pty::forward(pty_master)
    }

    return Ok(());
}

// TODO: move send_fd/receive_fd out

fn receive_fd(socket_fd: RawFd) -> Result<RawFd> {
    let mut buf = vec![0u8; 1];
    let mut csmg: CmsgSpace<([RawFd; 1])> = CmsgSpace::new();

    let msg = tryfmt!(
        recvmsg(
            socket_fd,
            &[IoVec::from_mut_slice(&mut buf)],
            Some(&mut csmg),
            MsgFlags::empty(),
        ),
        ""
    );

    if let Some(ControlMessage::ScmRights(fds)) = msg.cmsgs().next() {
        Ok(fds[0])
    } else {
        errfmt!("expected to receive a file descriptor")
    }
}

fn send_fd(socket_fd: RawFd, fd: RawFd) -> Result<()> {
    let fds = &[fd];
    tryfmt!(
        sendmsg(
            socket_fd,
            &[IoVec::from_slice(b"m")],
            &[ControlMessage::ScmRights(fds)],
            MsgFlags::empty(),
            None,
        ),
        ""
    );
    Ok(())
}

fn run_child(fs: fs::CntrFs, socket_fd: RawFd, opts: Options) -> Result<()> {
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

    let mountpoint = tryfmt!(TempDir::new("cntrfs"), "failed to create temporary mountpoint");
    let fuse_fd = tryfmt!(fs.mount(mountpoint.path()), "mount()");

    println!("mount at {:?}", mountpoint.path());

    tryfmt!(
        send_fd(socket_fd, fuse_fd),
        "failed to send fuse handle to parent"
    );

    #[allow(unreachable_patterns)]
    let _ = tryfmt!(cmd::exec(), "");
    Ok(())
}

pub fn run(opts: Options) -> Result<()> {
    tryfmt!(logging::init(), "failed to initialize logging");
    let (parent_sock, child_sock) = tryfmt!(
        socketpair(
            AddressFamily::Unix,
            SockType::Stream,
            None,
            SockFlag::empty(),
        ),
        "failed to open socketpair"
    );
    let cntr_fs = tryfmt!(fs::CntrFs::new(opts.mountpoint.as_str()), "cannot mount filesystem");

    let res = tryfmt!(pty::fork(), "fork failed");
    if let PtyFork::Parent { .. } = res {
        tryfmt!(unistd::close(child_sock), "failed to close child socket");
        run_parent(cntr_fs, parent_sock, res, opts)
    } else {
        tryfmt!(unistd::close(parent_sock), "failed to close parent socket");
        run_child(cntr_fs, child_sock, opts)
    }
}
