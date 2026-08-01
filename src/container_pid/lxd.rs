use std::process::Command;

use crate::container_pid::Container;
use crate::container_pid::Error;
use crate::container_pid::RawPid;
use crate::container_pid::cmd;

#[derive(Clone, Debug)]
pub(crate) struct Lxd {}

impl Container for Lxd {
    fn lookup(&self, container_id: &str) -> Result<RawPid, Error> {
        let output = Command::new("lxc")
            .args(["info", container_id])
            .output()
            .map_err(|source| Error::CommandFailedToRun {
                command: "lxc info".to_string(),
                source,
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(Error::CommandFailed {
                command: "lxc info".to_string(),
                status: output.status.to_string(),
                stderr: stderr.trim_end().to_string(),
            });
        }

        let lines = output.stdout.split(|&c| c == b'\n');
        let mut rows = lines.map(|line| {
            let cols: Vec<&[u8]> = line.splitn(2, |&c| c == b':').collect();
            cols
        });

        if let Some(pid_row) = rows.find(|cols| cols[0] == b"Pid") {
            if pid_row.len() != 2 {
                return Err(Error::UnexpectedOutput {
                    command: "lxc info".to_string(),
                    message: "unexpected format in 'Pid' field".to_string(),
                });
            }
            let pid = String::from_utf8_lossy(pid_row[1]);

            pid.trim_start()
                .parse::<RawPid>()
                .map_err(|source| Error::InvalidPid {
                    pid: pid.trim().to_string(),
                    runtime: "lxd",
                    container: container_id.to_string(),
                    source,
                })
        } else {
            Err(Error::UnexpectedOutput {
                command: "lxc info".to_string(),
                message: format!(
                    "no 'Pid' field found in output for container '{}'",
                    container_id
                ),
            })
        }
    }
    fn check_required_tools(&self) -> Result<(), Error> {
        if cmd::which("lxc").is_some() {
            Ok(())
        } else {
            Err(Error::RuntimeNotFound {
                runtime: "LXD",
                tool: "lxc",
            })
        }
    }
}
