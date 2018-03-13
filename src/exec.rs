
use cmd::Cmd;
use nix::unistd::Pid;
use types::{Error, Result};

pub fn exec(exe: Option<String>, args: Vec<String>) -> Result<()> {
    let cmd = tryfmt!(Cmd::new(exe, args, Pid::from_raw(1), None), "");
    tryfmt!(cmd.exec_chroot(), "failed to execute command in container");
    Ok(())
}
