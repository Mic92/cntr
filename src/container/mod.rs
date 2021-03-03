use nix::unistd::Pid;
use std::fmt::Debug;

use crate::types::{Error, Result};

mod command;
mod containerd;
mod docker;
mod lxc;
mod lxd;
mod nspawn;
mod podman;
mod process_id;
mod rkt;

use clap::arg_enum;

pub trait Container: Debug {
    fn lookup(&self, id: &str) -> Result<Pid>;
    fn check_required_tools(&self) -> Result<()>;
}

arg_enum! {
    #[derive(Debug)]
    #[allow(non_camel_case_types)]
    pub enum ContainerType {
        process_id,
        rkt,
        podman,
        docker,
        nspawn,
        lxc,
        lxd,
        containerd,
        command,
    }
}

fn default_order() -> Vec<Box<dyn Container>> {
    let containers: Vec<Box<dyn Container>> = vec![
        Box::new(process_id::ProcessId {}),
        Box::new(rkt::Rkt {}),
        Box::new(podman::Podman {}),
        Box::new(docker::Docker {}),
        Box::new(nspawn::Nspawn {}),
        Box::new(lxc::Lxc {}),
        Box::new(lxd::Lxd {}),
        Box::new(containerd::Containerd {}),
    ];
    containers
        .into_iter()
        .filter(|c| c.check_required_tools().is_ok())
        .collect()
}

pub fn lookup_container_type(name: &ContainerType) -> Box<dyn Container> {
    match name {
        ContainerType::process_id => Box::new(process_id::ProcessId {}),
        ContainerType::rkt => Box::new(rkt::Rkt {}),
        ContainerType::podman => Box::new(podman::Podman {}),
        ContainerType::docker => Box::new(docker::Docker {}),
        ContainerType::nspawn => Box::new(nspawn::Nspawn {}),
        ContainerType::lxc => Box::new(lxc::Lxc {}),
        ContainerType::lxd => Box::new(lxd::Lxd {}),
        ContainerType::containerd => Box::new(containerd::Containerd {}),
        ContainerType::command => Box::new(command::Command {}),
    }
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
