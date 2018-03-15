use nix::unistd::Pid;
use std::fmt::Debug;
use types::{Result, Error};

mod process_id;
mod docker;
mod nspawn;
mod rkt;
mod lxc;
mod lxd;

pub trait Container: Debug {
    fn lookup(&self, id: &str) -> Result<Pid>;
    fn check_required_tools(&self) -> Result<()>;
}

pub const AVAILABLE_CONTAINER_TYPES: &[&str] = &["process_id", "rkt", "docker", "nspawn", "lxc", "lxd"];

fn default_order() -> Vec<Box<Container>> {
    let containers: Vec<Box<Container>> = vec![
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

pub fn lookup_container_type(name: &str) -> Option<Box<Container>> {
    Some(match name {
        "process_id" => Box::new(process_id::ProcessId {}),
        "rkt" => Box::new(rkt::Rkt {}),
        "docker" => Box::new(docker::Docker {}),
        "nspawn" => Box::new(nspawn::Nspawn {}),
        "lxc" => Box::new(lxc::Lxc {}),
        "lxd" => Box::new(lxd::Lxd {}),
        _ => return None,
    })
}

pub fn lookup_container_pid(container_id: &str, container_types: &[Box<Container>]) -> Result<Pid> {
    for c in container_types {
        try!(c.check_required_tools());
    }
    let fallback: Vec<Box<Container>> = default_order();
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
