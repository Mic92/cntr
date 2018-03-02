

use container::Container;
use libc::pid_t;
use nix::unistd::Pid;
use std::process::Command;
use types::{Error, Result};

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
                stderr.trim_right()
            ));
        }

        let fields: Vec<&[u8]> = output.stdout.splitn(2, |c| *c == b'=').collect();
        assert!(fields.len() == 2);

        let pid = String::from_utf8_lossy(fields[1]);

        Ok(Pid::from_raw(tryfmt!(
            pid.trim_right().parse::<pid_t>(),
            "expected valid process id from {}, got: {}",
            command,
            pid
        )))
    }
    fn check_required_tools(&self) -> Result<()> {
        tryfmt!(
            Command::new("machinectl").arg("--version").output(),
            "cannot execute `machinectl`"
        );
        Ok(())
    }
}
