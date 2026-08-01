use crate::container_pid::Container;
use crate::container_pid::Error;
use crate::container_pid::RawPid;
use crate::container_pid::cmd;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

#[derive(Clone, Debug)]
pub(crate) struct Nspawn {}

impl Container for Nspawn {
    fn lookup(&self, container_id: &str) -> Result<RawPid, Error> {
        let stdout = cmd::output("machinectl", &["show", "--property=Leader", container_id])?;

        let fields: Vec<&[u8]> = stdout.splitn(2, |c| *c == b'=').collect();
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
