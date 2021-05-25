use libc::pid_t;
use nix::unistd::Pid;
use simple_error::try_with;
use std::fs::File;
use std::io::prelude::*;
use std::os::unix::process::CommandExt;
use std::process::Command;

use crate::capabilities;
use crate::cmd::Cmd;
use crate::result::Result;

pub const SETCAP_EXE: &str = "/.cntr/cntr-exec";
pub const EXEC_PID_FILE: &str = "/.cntr/pid";

pub fn exec(exe: Option<String>, mut args: Vec<String>, has_setcap: bool) -> Result<()> {
    if !has_setcap {
        let has_chroot = try_with!(
            capabilities::has_chroot(),
            "failed to check if process has chroot capability"
        );
        if !has_chroot {
            if let Some(e) = exe {
                args.insert(0, e);
            }
            try_with!(
                Err(Command::new(SETCAP_EXE).args(args).exec()),
                "failed to start capability wrapper"
            );
            // BUG!
            return Ok(());
        }
    }

    let mut f = try_with!(
        File::open(EXEC_PID_FILE),
        "failed to open {}",
        EXEC_PID_FILE
    );

    let mut pid_string = String::new();
    try_with!(
        f.read_to_string(&mut pid_string),
        "failed to read {}",
        EXEC_PID_FILE
    );

    let pid = try_with!(
        pid_string.parse::<pid_t>(),
        "failed to parse pid {} in pid file {}",
        pid_string,
        EXEC_PID_FILE
    );

    let cmd = Cmd::new(exe, args, Pid::from_raw(pid), None)?;
    try_with!(cmd.exec_chroot(), "failed to execute command in container");
    Ok(())
}
