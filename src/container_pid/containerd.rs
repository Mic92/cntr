use std::process::Command;

use crate::container_pid::Container;
use crate::container_pid::Error;
use crate::container_pid::RawPid;
use crate::container_pid::cmd;

#[derive(Clone, Debug)]
pub(crate) struct Containerd {}

impl Container for Containerd {
    fn lookup(&self, container_id: &str) -> Result<RawPid, Error> {
        let output = Command::new("ctr")
            .args(["task", "list"])
            .output()
            .map_err(|source| Error::CommandFailedToRun {
                command: "ctr task list".to_string(),
                source,
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(Error::CommandFailed {
                command: "ctr task list".to_string(),
                status: output.status.to_string(),
                stderr: stderr.trim_end().to_string(),
            });
        }

        // $ ctr task list
        // TASK    PID      STATUS
        // v2      17515    RUNNING
        // v1      14602    RUNNING
        let mut lines = output.stdout.split(|&c| c == b'\n');
        lines.next(); // skip header
        let pid_str = lines.find_map(|line| {
            let line_str = String::from_utf8_lossy(line);
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
            Some(pid_str) => pid_str
                .parse::<RawPid>()
                .map_err(|source| Error::InvalidPid {
                    pid: pid_str,
                    runtime: "containerd",
                    container: container_id.to_string(),
                    source,
                }),
            None => Err(Error::ContainerNotFound {
                container: container_id.to_string(),
                message: "no containerd task found with this id".to_string(),
            }),
        }
    }
    fn check_required_tools(&self) -> Result<(), Error> {
        if cmd::which("ctr").is_some() {
            Ok(())
        } else {
            Err(Error::RuntimeNotFound {
                runtime: "containerd",
                tool: "ctr",
            })
        }
    }
}
