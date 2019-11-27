use cmd;
use container::Container;
use libc::pid_t;
use nix::unistd::Pid;
use std::fs::{self, File};
use std::io::BufReader;
use std::io::prelude::*;
use std::process::Command;
use types::{Error, Result};

#[derive(Clone, Debug)]
pub struct Rkt {}

fn find_child_processes(parent_pid: &str) -> Result<Pid> {
    let dir = tryfmt!(fs::read_dir("/proc"), "failed to read /proc directory");

    for entry in dir {
        let entry = tryfmt!(entry, "error while reading /proc");
        let status_path = entry.path().join("status");
        if let Ok(file) = File::open(status_path.clone()) {
            // ignore if process exits before we can read it
            let reader = BufReader::new(file);
            for line in reader.lines() {
                let line = tryfmt!(line, "could not read {}", status_path.display());
                let columns: Vec<&str> = line.splitn(2, '\t').collect();
                assert!(columns.len() == 2);
                if columns[0] == "PPid:" && columns[1] == parent_pid {
                    let pid = tryfmt!(
                        entry.file_name().to_string_lossy().parse::<pid_t>(),
                        "read invalid pid from proc: '{}'",
                        columns[1]
                    );
                    return Ok(Pid::from_raw(pid));
                }
            }
        }
    }

    errfmt!(format!("no child process found for pid {}", parent_pid))
}

impl Container for Rkt {
    fn lookup(&self, container_id: &str) -> Result<Pid> {
        let command = format!("rkt status {}", container_id);
        let output = tryfmt!(
            Command::new("rkt").args(&["status", container_id]).output(),
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

        let lines = output.stdout.split(|&c| c == b'\n');
        let mut rows = lines.map(|line| {
            let cols: Vec<&[u8]> = line.splitn(2, |&c| c == b'=').collect();
            cols
        });
        if let Some(pid_row) = rows.find(|cols| cols[0] == b"pid") {
            assert!(pid_row.len() == 2);
            let ppid = String::from_utf8_lossy(pid_row[1]);
            Ok(tryfmt!(
                find_child_processes(&ppid),
                "could not find container process belonging to rkt container '{}'",
                container_id
            ))
        } else {
            let stdout = String::from_utf8_lossy(&output.stdout);
            errfmt!(format!(
                "expected to find `pid=` field in output of '{}', got: {}",
                command,
                stdout
            ))
        }
    }
    fn check_required_tools(&self) -> Result<()> {
        if cmd::which("rkt").is_some() {
            Ok(())
        } else {
            errfmt!("rkt not found")
        }
    }
}
