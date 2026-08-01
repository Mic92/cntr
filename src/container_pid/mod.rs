use std::fmt::Debug;
use std::fmt::Write;

/// A raw process ID as used by the kernel.
pub type RawPid = i32;

mod cmd;
mod command;
mod containerd;
mod docker;
mod errors;
mod kubernetes;
mod lxc;
mod lxd;
mod nspawn;
mod podman;
mod process_id;

pub use errors::Error;

pub trait Container: Debug {
    fn lookup(&self, id: &str) -> Result<RawPid, Error>;
    fn check_required_tools(&self) -> Result<(), Error>;
}

fn default_order() -> Vec<Box<dyn Container>> {
    let containers: Vec<Box<dyn Container>> = vec![
        Box::new(process_id::ProcessId {}),
        Box::new(podman::Podman {}),
        Box::new(docker::Docker {}),
        Box::new(nspawn::Nspawn {}),
        Box::new(lxc::Lxc {}),
        Box::new(lxd::Lxd {}),
        Box::new(containerd::Containerd {}),
        Box::new(kubernetes::Kubernetes {}),
    ];
    containers
        .into_iter()
        .filter(|c| c.check_required_tools().is_ok())
        .collect()
}

pub fn lookup_container_type(name: &str) -> Option<Box<dyn Container>> {
    Some(match name {
        "process_id" => Box::new(process_id::ProcessId {}),
        "podman" => Box::new(podman::Podman {}),
        "docker" => Box::new(docker::Docker {}),
        "nspawn" => Box::new(nspawn::Nspawn {}),
        "lxc" => Box::new(lxc::Lxc {}),
        "lxd" => Box::new(lxd::Lxd {}),
        "containerd" => Box::new(containerd::Containerd {}),
        "command" => Box::new(command::Command {}),
        "kubernetes" => Box::new(kubernetes::Kubernetes {}),
        _ => return None,
    })
}

pub fn lookup_container_pid(
    container_id: &str,
    container_types: &[Box<dyn Container>],
) -> Result<RawPid, Error> {
    for c in container_types {
        c.check_required_tools()?;
    }
    let fallback: Vec<Box<dyn Container>> = default_order();
    let types = if container_types.is_empty() {
        fallback.as_slice()
    } else {
        container_types
    };

    let mut tried = String::new();
    for t in types {
        match t.lookup(container_id) {
            Ok(pid) => return Ok(pid),
            Err(e) => {
                let _ = write!(tried, "\n  - {:?}: {}", t, crate::errors::format_chain(&e));
            }
        };
    }

    Err(Error::NoRuntimeMatched {
        container: container_id.to_string(),
        tried,
    })
}
