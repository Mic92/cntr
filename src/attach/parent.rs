use fs;
use ipc;
use mountns;
use nix::pty::PtyMaster;
use nix::sys::signal::{self, Signal};
use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};
use nix::unistd;
use nix::unistd::Pid;
use pty;
use socket_proxy::{self, SocketProxy, Listener};
use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
use std::os::unix::io::IntoRawFd;
use std::os::unix::prelude::*;
use std::path::PathBuf;
use std::process;
use std::str;
use types::{Error, Result};
use void::Void;

fn setup_pty(mount_ready_sock: &ipc::Socket) -> Result<(PtyMaster, usize)> {
    let (sockets_bytes, mut fd) = tryfmt!(
        mount_ready_sock.receive(255, 1),
        "failed to receive pty file descriptor"
    );
    assert!(fd.len() == 1);
    let master = unsafe { PtyMaster::from_raw_fd(fd.pop().unwrap().into_raw_fd()) };

    let sockets_str = tryfmt!(
        str::from_utf8(sockets_bytes.as_slice()),
        "failed to decode message from child"
    );

    let n_sockets = tryfmt!(
        sockets_str.parse::<usize>(),
        "received socket number string is not a number"
    );

    Ok((master, n_sockets))
}

fn setup_proxy(mount_ready_sock: &ipc::Socket, n_sockets: usize) -> Result<SocketProxy> {
    let mut listeners = Vec::new();
    listeners.reserve(n_sockets);

    for i in 1..n_sockets {
        let (path, mut fd) = tryfmt!(
            mount_ready_sock.receive(255, 1),
            "failed to receive unix socket file descriptor"
        );
        assert!(fd.len() == 1);

        listeners.push(Listener {
            address: PathBuf::from(OsString::from_vec(path)),
            socket: fd.pop().unwrap(),
        });
    }


    let proxy = tryfmt!(
        socket_proxy::start(listeners),
        "failed to start socket proxy"
    );

    Ok(proxy)
}

pub fn run(pid: Pid, mount_ready_sock: &ipc::Socket, fs: fs::CntrFs) -> Result<Void> {
    let ns = tryfmt!(
        mountns::MountNamespace::receive(mount_ready_sock),
        "failed to receive mount namespace from child"
    );

    let sessions = fs.spawn_sessions();

    let (master, n_sockets) = try!(setup_pty(mount_ready_sock));

    let proxy = try!(setup_proxy(mount_ready_sock, n_sockets));

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
                drop(proxy);
                return tryfmt!(Err(e), "waitpid failed");
            }
        };
    }
}
