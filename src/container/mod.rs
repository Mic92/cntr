use nix::unistd::Pid;
use std::fmt::Debug;
use types::{Error, Result};

mod command;
mod docker;
mod lxc;
mod lxd;
mod nspawn;
mod process_id;
mod rkt;

pub trait Container: Debug {
    fn lookup(&self, id: &str) -> Result<Pid>;
    fn check_required_tools(&self) -> Result<()>;
}

pub const AVAILABLE_CONTAINER_TYPES: &[&str] = &[
    "process_id",
    "rkt",
    "docker",
    "nspawn",
    "lxc",
    "lxd",
    "command",
];

fn default_order() -> Vec<Box<dyn Container>> {
    let containers: Vec<Box<dyn Container>> = vec![
        Box::new(process_id::ProcessId {}),
        Box::new(rkt::Rkt {}),
        Box::new(docker::Docker {}),
        Box::new(nspawn::Nspawn {}),
        Box::new(lxc::Lxc {}),
        Box::new(lxd::Lxd {}),
    ];
    containers
        .into_iter()
        .filter(|c| c.check_required_tools().is_ok())
        .collect()
}

pub fn lookup_container_type(name: &str) -> Option<Box<dyn Container>> {
    Some(match name {
        "process_id" => Box::new(process_id::ProcessId {}),
        "rkt" => Box::new(rkt::Rkt {}),
        "docker" => Box::new(docker::Docker {}),
        "nspawn" => Box::new(nspawn::Nspawn {}),
        "lxc" => Box::new(lxc::Lxc {}),
        "lxd" => Box::new(lxd::Lxd {}),
        "command" => Box::new(command::Command {}),
        _ => return None,
    })
}

pub fn lookup_container_pid(
    container_id: &str,
    container_types: &[Box<dyn Container>],
) -> Result<Pid> {
    for c in container_types {
        c.check_required_tools()?;
    }
    let fallback: Vec<Box<dyn Container>> = default_order();
    let types = if container_types.is_empty() {
        fallback.as_slice()
    } else {
        container_types
    };

    let mut message = String::from("no suitable container found, got the following errors:");
    for t in types {
        match t.lookup(container_id) {
            Ok(pid) => return Ok(pid),
            Err(e) => {
                message += &format!("\n  - {:?}: {}", t, e);
            }
        };
    }

    errfmt!(message)
}
