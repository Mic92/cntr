use crate::container_pid::Container;
use crate::container_pid::Error;
use crate::container_pid::RawPid;
use crate::container_pid::cmd;
use crate::container_pid::docker::parse_docker_output;
use alloc::vec;

#[derive(Clone, Debug)]
pub(crate) struct Podman {}

impl Container for Podman {
    fn lookup(&self, container_id: &str) -> Result<RawPid, Error> {
        let cmd = vec![
            "podman",
            "inspect",
            "--format",
            "{{.State.Running}};{{.State.Pid}}",
            container_id,
        ];
        parse_docker_output("podman", cmd.as_slice(), container_id)
    }
    fn check_required_tools(&self) -> Result<(), Error> {
        if cmd::which("podman").is_some() {
            Ok(())
        } else {
            Err(Error::RuntimeNotFound {
                runtime: "podman",
                tool: "podman",
            })
        }
    }
}
