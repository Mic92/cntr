use libc::pid_t;
use nix::unistd::Pid;
use std::process::Command;

use crate::cmd;
use crate::container::Container;
use crate::types::{Error, Result};

#[derive(Clone, Debug)]
pub struct Nspawn {}

impl Container for Nspawn {
    fn lookup(&self, container_id: &str) -> Result<Pid> {
        let command = format!("machinectl show --property=Leader {}", container_id);
        let output = tryfmt!(
            Command::new("machinectl")
                .args(&["show", "--property=Leader", container_id])
                .output(),
            "Running '{}' failed",
            command
        );

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return errfmt!(format!(
                "Failed to list containers. '{}' exited with {}: {}",
                command,
                output.status,
                stderr.trim_end()
            ));
        }

        let fields: Vec<&[u8]> = output.stdout.splitn(2, |c| *c == b'=').collect();
        assert!(fields.len() == 2);

        let pid = String::from_utf8_lossy(fields[1]);

        Ok(Pid::from_raw(tryfmt!(
            pid.trim_end().parse::<pid_t>(),
            "expected valid process id from {}, got: {}",
            command,
            pid
        )))
    }
    fn check_required_tools(&self) -> Result<()> {
        if cmd::which("machinectl").is_some() {
            Ok(())
        } else {
            errfmt!("machinectl not found")
        }
    }
}
