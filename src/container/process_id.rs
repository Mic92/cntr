

use container::Container;
use libc::pid_t;
use std::fs;
use std::io::ErrorKind;
use std::path::PathBuf;
use types::{Result, Error};
use unistd::Pid;

#[derive(Clone, Debug)]
pub struct ProcessId {}

impl Container for ProcessId {
    fn lookup(&self, container_id: &str) -> Result<Pid> {
        let pid = match container_id.parse::<pid_t>() {
            Err(e) => tryfmt!(Err(e), "not a valid pid: `{}`", container_id),
            Ok(v) => v,
        };
        let mut path = PathBuf::from("/proc");
        path.push(pid.to_string());
        match fs::metadata(path) {
            Err(e) => {
                if e.kind() == ErrorKind::NotFound {
                    errfmt!(format!("no process with pid {} found", pid))
                } else {
                    tryfmt!(Err(e), "could not lookup process {}", pid)
                }
            }
            Ok(_) => Ok(Pid::from_raw(pid)),
        }
    }
    fn check_required_tools(&self) -> Result<()> {
        Ok(())
    }
}
