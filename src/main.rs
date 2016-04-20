extern crate argparse;
extern crate libc;
extern crate nix;
#[macro_use]
extern crate log;
extern crate core;

use argparse::{ArgumentParser, Store};
use std::io::Write;
use std::process;
use types::{Error, Result};
use std::thread;

#[macro_use]
mod types;
mod namespace;
mod cgroup;
mod pty;
mod logging;
mod cmd;
mod trace;
mod sigstr;
mod fuse;

struct Options {
    pid: libc::pid_t,
}

fn parse_args() -> Options {
    let mut options = Options { pid: 0 };
    {
        let mut ap = ArgumentParser::new();
        ap.set_description("Enter container");
        ap.refer(&mut options.pid)
          .add_argument("pid", Store, "target pid");
        ap.parse_args_or_exit();
    }
    return options;
}

fn run(opts: Options) -> Result<()> {
    tryfmt!(logging::init(), "failed to initialize logging");
    let res = tryfmt!(pty::fork(), "fork failed");
    if let pty::PtyFork::Parent { pid, .. } = res {
        tryfmt!(cgroup::move_to(pid, opts.pid), "failed to change cgroup");
        // let mut mask = SigSet::empty();
        // mask.add(signal::SIGUSR1).unwrap();
        // let mut sfd = SignalFd::with_flags(&mask, SFD_NONBLOCK).unwrap();
        // sfd.read_signal();
        tryfmt!(trace::install(pid), "failed to initialize seccomp sandbox");
        let child = thread::spawn(move || {
            if let pty::PtyFork::Parent { ref pty_master, .. } = res {
                pty::forward(pty_master)
            }
        });
        tryfmt!(trace::dispatch(), "failed to dispatch");
        if let Err(_) = child.join() {
            return errfmt!("pty thread died");
        };
        return Ok(());
    }
    return cmd::exec(opts.pid);
}

fn main() {
    let opts = parse_args();
    match run(opts) {
        Err(err) => {
            let _ = writeln!(&mut std::io::stderr(), "{}", err);
            process::exit(1);
        }
        _ => {}
    };
}
