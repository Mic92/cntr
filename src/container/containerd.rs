use libc::pid_t;
use nix::unistd::Pid;
use std::process::Command;

use crate::cmd;
use crate::container::Container;
use crate::types::{Error, Result};

#[derive(Clone, Debug)]
pub struct Containerd {}

impl Container for Containerd {
    fn lookup(&self, container_id: &str) -> Result<Pid> {
        let command = "ctr task list";
        let output = tryfmt!(
            Command::new("ctr").args(&["task", "list"]).output(),
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

        // $ ctr task list
        // TASK    PID      STATUS
        // v2      17515    RUNNING
        // v1      14602    RUNNING
        let mut lines = output.stdout.split(|&c| c == b'\n');
        lines.next(); // skip header
        let pid_str = lines.find_map(|line| {
            let line_str = String::from_utf8_lossy(&line);
            let cols = line_str.split_whitespace().collect::<Vec<&str>>();
            if cols.len() != 3 {
                return None;
            }

            if cols[0] == container_id {
                Some(String::from(cols[1]))
            } else {
                None
            }
        });
        match pid_str {
            Some(pid_str) => {
                let pid = tryfmt!(
                    pid_str.parse::<pid_t>(),
                    "read invalid pid from ctr task list: '{}'",
                    pid_str
                );
                Ok(Pid::from_raw(pid))
            }
            None => {
                errfmt!(format!("No container with id {} found", container_id))
            }
        }
    }
    fn check_required_tools(&self) -> Result<()> {
        if cmd::which("ctr").is_some() {
            Ok(())
        } else {
            errfmt!("ctr not found")
        }
    }
}
