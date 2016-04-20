use libc;
use nix;
use nix::errno;
use sigstr::Signal;
use nix::sys::wait::{WaitStatus, wait, waitpid};
use nix::sys::ptrace::*;
use nix::sys::ptrace::ptrace::*;
use std::ptr;
use types::{Error, Result};

pub fn install(pid: libc::pid_t) -> Result<()> {
    let status = tryfmt!(waitpid(pid, None), "process died prematurely");
    match status {
        WaitStatus::Exited(_, rc) => {
            return errfmt!(format!("process exited prematurely with {}", rc));
        }
        WaitStatus::Signaled(_, signal, _) => {
            return errfmt!(format!("process was terminated with signal {}",
                                   Signal { n: signal }));
        }
        WaitStatus::Continued(_) => {
            return errfmt!(format!("BUG: process was continued by someone"));
        }
        WaitStatus::StillAlive => {
            return errfmt!(format!("process should be stopped"));
        }
        WaitStatus::Stopped(_, _) => {}
    }

    let opts = PTRACE_O_TRACESECCOMP | PTRACE_O_TRACESYSGOOD | PTRACE_O_TRACEFORK |
               PTRACE_O_TRACEVFORK | PTRACE_O_TRACECLONE | PTRACE_O_TRACEEXEC |
               PTRACE_O_TRACEVFORKDONE | PTRACE_O_TRACEEXIT;
    tryfmt!(ptrace_setoptions(pid, opts), "failed to ptrace process");
    tryfmt!(ptrace(PTRACE_CONT, pid, ptr::null_mut(), ptr::null_mut()),
            "failed to resume tracee");
    Ok(())
}

pub fn me() -> nix::Result<libc::c_long> {
    ptrace(PTRACE_TRACEME, 0, ptr::null_mut(), ptr::null_mut())
}

pub fn dispatch() -> Result<()> {
    loop {
        match wait() {
            Err(nix::Error::Sys(errno::ECHILD)) => return Ok(()),
            Ok(WaitStatus::Stopped(pid, _)) => {
                ptrace(PTRACE_CONT, pid, ptr::null_mut(), ptr::null_mut());
            }
            _ => {}
        };
    }
}
