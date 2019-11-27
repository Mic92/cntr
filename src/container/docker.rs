use cmd;
use container::Container;
use libc::pid_t;
use nix::unistd::Pid;
use std::process::Command;
use types::{Error, Result};

#[derive(Clone, Debug)]
pub struct Docker {}

impl Container for Docker {
    fn lookup(&self, container_id: &str) -> Result<Pid> {
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

        let output = tryfmt!(
            Command::new(&command[0]).args(&command[1..]).output(),
            "Running '{}' failed",
            command.join(" ")
        );

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return errfmt!(format!(
                "Failed to list containers. '{}' exited with {}: {}",
                command.join(" "),
                output.status,
                stderr.trim_end()
            ));
        }

        let fields: Vec<&[u8]> = output.stdout.splitn(2, |c| *c == b';').collect();
        assert!(fields.len() == 2);

        if fields[0] != b"true" {
            return errfmt!(format!("container '{}' is not running", container_id,));
        }

        let pid = String::from_utf8_lossy(fields[1]);

        Ok(Pid::from_raw(tryfmt!(
            pid.trim_end().parse::<pid_t>(),
            "expected valid process id from '{}', got: {}",
            command.join(" "),
            pid
        )))
    }
    fn check_required_tools(&self) -> Result<()> {
        if cmd::which("docker-pid").is_some() || cmd::which("docker").is_some() {
            return Ok(());
        }

        errfmt!("Neither docker or docker-pid was found")
    }
}
