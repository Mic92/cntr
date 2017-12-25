use libc::pid_t;
use std::process::Command;
use types::{Error, Result};
use unistd::Pid;

pub fn lookup_process_id(container_id: &str) -> Result<Pid> {
    let output = tryfmt!(
        Command::new("docker")
            .args(
                &[
                    "inspect",
                    "--format",
                    "{{.State.Status}};{{.State.Pid}}",
                    container_id,
                ],
            )
            .output(),
        "Running 'docker inspect' failed"
    );

    if !output.status.success() {
        if output.stderr.starts_with(b"Error: No such object") {
            return errfmt!("no such container found");
        }
        let stderr = String::from_utf8_lossy(&output.stderr);
        return errfmt!(format!(
            "Failed to list containers. 'docker inspect' exited with {}: {}",
            output.status,
            stderr
        ));
    }

    let fields: Vec<&[u8]> = output.stdout.splitn(2, |c| *c == b';').collect();
    assert!(fields.len() == 2);

    if fields[0] != b"running" {
        let state = String::from_utf8_lossy(fields[0]);
        return errfmt!(format!(
            "container '{}' is not running, got state: {}",
            container_id,
            state
        ));
    }

    let pid = String::from_utf8_lossy(fields[1]);

    Ok(Pid::from_raw(tryfmt!(
        pid.trim_right().parse::<pid_t>(),
        "expected pid from docker inspect, got: {}",
        pid
    )))
}
