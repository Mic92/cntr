use types::{Result, Error};
use unistd::Pid;

mod process_id;
mod docker;

#[derive(Debug, Clone)]
pub enum ContainerType {
    Docker,
    ProcessId,
}

impl ContainerType {
    fn lookup(&self, id: &str) -> Result<Pid> {
        match *self {
            ContainerType::Docker => docker::lookup_process_id(id),
            ContainerType::ProcessId => process_id::lookup_process_id(id),
        }
    }
}

const DEFAULT_ORDER: &[ContainerType] = &[ContainerType::ProcessId, ContainerType::Docker];

pub fn lookup_container_pid(
    container_id: &str,
    container_type: Option<ContainerType>,
) -> Result<Pid> {
    let types = container_type.map_or(DEFAULT_ORDER.to_vec(), |t| vec![t]);

    let mut message = String::from("no suitable container found, got the following errors:");
    for t in types {
        match t.lookup(container_id) {
            Ok(pid) => return Ok(pid),
            Err(e) => {
                message += &format!("\n{:?}: {}", t, e);
            }
        };
    }

    errfmt!(message)
}
