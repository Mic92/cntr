use nix::unistd::Pid;

use crate::cmd;
use crate::container::docker::parse_docker_output;
use crate::container::Container;
use crate::types::{Error, Result};

#[derive(Clone, Debug)]
pub struct Podman {}

impl Container for Podman {
    fn lookup(&self, container_id: &str) -> Result<Pid> {
        let cmd = vec![
            "podman",
            "inspect",
            "--format",
            "{{.State.Running}};{{.State.Pid}}",
            container_id,
        ];
        parse_docker_output(cmd.as_slice(), container_id)
    }
    fn check_required_tools(&self) -> Result<()> {
        if cmd::which("podman").is_some() {
            Ok(())
        } else {
            errfmt!("podman not found")
        }
    }
}
