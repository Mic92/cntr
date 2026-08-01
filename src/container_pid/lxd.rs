use crate::container_pid::Container;
use crate::container_pid::Error;
use crate::container_pid::RawPid;
use crate::container_pid::cmd;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

#[derive(Clone, Debug)]
pub(crate) struct Lxd {}

impl Container for Lxd {
    fn lookup(&self, container_id: &str) -> Result<RawPid, Error> {
        let stdout = cmd::output("lxc", &["info", container_id])?;

        let lines = stdout.split(|&c| c == b'\n');
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
