use std::process::Command;

use crate::container_pid::Container;
use crate::container_pid::Error;
use crate::container_pid::RawPid;
use crate::container_pid::cmd;

#[derive(Clone, Debug)]
pub(crate) struct Docker {}

pub(crate) fn parse_docker_output(
    runtime: &'static str,
    cmd: &[&str],
    container_id: &str,
) -> Result<RawPid, Error> {
    let cmd_str = cmd.join(" ");
    let output = Command::new(cmd[0])
        .args(&cmd[1..])
        .output()
        .map_err(|source| Error::CommandFailedToRun {
            command: cmd_str.clone(),
            source,
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Error::CommandFailed {
            command: cmd_str,
            status: output.status.to_string(),
            stderr: stderr.trim_end().to_string(),
        });
    }

    let fields: Vec<&[u8]> = output.stdout.splitn(2, |c| *c == b';').collect();
    if fields.len() != 2 {
        return Err(Error::UnexpectedOutput {
            command: cmd_str,
            message: format!(
                "expected '<running>;<pid>' for container '{}'",
                container_id
            ),
        });
    }

    if fields[0] != b"true" {
        return Err(Error::NotRunning(container_id.to_string()));
    }

    let pid = String::from_utf8_lossy(fields[1]);

    pid.trim_end()
        .parse::<RawPid>()
        .map_err(|source| Error::InvalidPid {
            pid: pid.trim_end().to_string(),
            runtime,
            container: container_id.to_string(),
            source,
        })
}

impl Container for Docker {
    fn lookup(&self, container_id: &str) -> Result<RawPid, Error> {
        let command = if cmd::which("docker-pid").is_some() {
            vec!["docker-pid", container_id]
        } else {
            vec![
                "docker",
                "inspect",
                "--format",
                "{{.State.Running}};{{.State.Pid}}",
                container_id,
            ]
        };
        parse_docker_output("docker", command.as_slice(), container_id)
    }
    fn check_required_tools(&self) -> Result<(), Error> {
        if cmd::which("docker-pid").is_some() || cmd::which("docker").is_some() {
            return Ok(());
        }

        Err(Error::RuntimeNotFound {
            runtime: "docker",
            tool: "docker",
        })
    }
}
