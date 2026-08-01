use crate::container_pid::Container;
use crate::container_pid::Error;
use crate::container_pid::RawPid;
use crate::container_pid::cmd;

#[derive(Clone, Debug)]
pub(crate) struct Lxc {}

impl Container for Lxc {
    fn lookup(&self, container_id: &str) -> Result<RawPid, Error> {
        let stdout = cmd::output(
            "lxc-info",
            &["--no-humanize", "--pid", "--name", container_id],
        )?;

        let pid = String::from_utf8_lossy(&stdout);

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
