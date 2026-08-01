use std::process::Command;

use crate::container_pid::Container;
use crate::container_pid::Error;
use crate::container_pid::RawPid;
use crate::container_pid::cmd;

#[derive(Clone, Debug)]
pub(crate) struct Lxc {}

impl Container for Lxc {
    fn lookup(&self, container_id: &str) -> Result<RawPid, Error> {
        let output = Command::new("lxc-info")
            .args(["--no-humanize", "--pid", "--name", container_id])
            .output()
            .map_err(|source| Error::CommandFailedToRun {
                command: "lxc-info".to_string(),
                source,
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(Error::CommandFailed {
                command: "lxc-info".to_string(),
                status: output.status.to_string(),
                stderr: stderr.trim_start().to_string(),
            });
        }

        let pid = String::from_utf8_lossy(&output.stdout);

        pid.trim_start()
            .parse::<RawPid>()
            .map_err(|source| Error::InvalidPid {
                pid: pid.trim().to_string(),
                runtime: "lxc",
                container: container_id.to_string(),
                source,
            })
    }
    fn check_required_tools(&self) -> Result<(), Error> {
        if cmd::which("lxc-info").is_some() {
            Ok(())
        } else {
            Err(Error::RuntimeNotFound {
                runtime: "LXC",
                tool: "lxc-info",
            })
        }
    }
}
