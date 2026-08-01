use std::process::Command;

use crate::container_pid::Container;
use crate::container_pid::Error;
use crate::container_pid::RawPid;
use crate::container_pid::cmd;

#[derive(Clone, Debug)]
pub(crate) struct Nspawn {}

impl Container for Nspawn {
    fn lookup(&self, container_id: &str) -> Result<RawPid, Error> {
        let output = Command::new("machinectl")
            .args(["show", "--property=Leader", container_id])
            .output()
            .map_err(|source| Error::CommandFailedToRun {
                command: "machinectl show".to_string(),
                source,
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(Error::CommandFailed {
                command: "machinectl show".to_string(),
                status: output.status.to_string(),
                stderr: stderr.trim_end().to_string(),
            });
        }

        let fields: Vec<&[u8]> = output.stdout.splitn(2, |c| *c == b'=').collect();
        if fields.len() != 2 {
            return Err(Error::UnexpectedOutput {
                command: "machinectl show".to_string(),
                message: format!("unexpected output format for container '{}'", container_id),
            });
        }

        let pid = String::from_utf8_lossy(fields[1]);

        pid.trim_end()
            .parse::<RawPid>()
            .map_err(|source| Error::InvalidPid {
                pid: pid.trim().to_string(),
                runtime: "machinectl",
                container: container_id.to_string(),
                source,
            })
    }
    fn check_required_tools(&self) -> Result<(), Error> {
        if cmd::which("machinectl").is_some() {
            Ok(())
        } else {
            Err(Error::RuntimeNotFound {
                runtime: "systemd-nspawn",
                tool: "machinectl",
            })
        }
    }
}
