use libc;
use nix;
use nix::errno;
use nix::sys::ptrace::ptrace::*;
use nix::sys::ptrace::*;
use nix::sys::wait::{wait, waitpid, WaitStatus};
use sigstr;
use simple_error::{bail, try_with};
use std::ptr;
use types::Result;

pub fn install(pid: libc::pid_t) -> Result<()> {
    let status = try_with!(waitpid(pid, None), "process died prematurely");
    match status {
        WaitStatus::Exited(_, rc) => {
            bail!("process exited prematurely with {}", rc);
        }
        WaitStatus::Signaled(_, signal, _) => {
            bail!(
                "process was terminated with signal {:}",
                sigstr::Signal { n: signal }
            );
        }
        WaitStatus::Continued(_) => {
            bail!("BUG: process was continued by someone");
        }
        WaitStatus::StillAlive => {
            bail!("process should be stopped");
        }
        WaitStatus::Stopped(_, _) => {}
    }

    let opts = PTRACE_O_TRACESECCOMP
        | PTRACE_O_TRACESYSGOOD
        | PTRACE_O_TRACEFORK
        | PTRACE_O_TRACEVFORK
        | PTRACE_O_TRACECLONE
        | PTRACE_O_TRACEEXEC
        | PTRACE_O_TRACEVFORKDONE
        | PTRACE_O_TRACEEXIT;
    try_with!(ptrace_setoptions(pid, opts), "failed to ptrace process");
    try_with!(
        ptrace(PTRACE_CONT, pid, ptr::null_mut(), ptr::null_mut()),
        "failed to resume tracee"
    );
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
