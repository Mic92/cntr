use fs;
use ipc;
use mountns;
use nix::sys::signal::{self, Signal};
use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};
use nix::unistd;
use nix::unistd::Pid;
use pty;
use std::os::unix::prelude::RawFd;
use std::process;
use types::{Error, Result};
use void::Void;

pub fn run(pid: Pid, mount_ready_sock: &ipc::Socket, fs: fs::CntrFs) -> Result<Void> {
    let ns = tryfmt!(
        mountns::MountNamespace::receive(mount_ready_sock),
        "failed to receive mount namespace from child"
    );

    let sessions = fs.spawn_sessions();

    let mut cmsgspace = cmsg_space!([RawFd; 1]);
    let (_, mut fds) = tryfmt!(
        mount_ready_sock.receive(1, &mut cmsgspace),
        "failed to receive pty file descriptor"
    );
    assert!(fds.len() == 1);
    let fd = fds.pop().unwrap();

    ns.cleanup();

    loop {
        tryfmt!(
            pty::forward(&fd),
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
