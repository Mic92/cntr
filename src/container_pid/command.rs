use std::path::{Path, PathBuf};

use crate::fsutil;

use crate::container_pid::Container;
use crate::container_pid::Error;
use crate::container_pid::RawPid;

#[derive(Clone, Debug)]
pub(crate) struct Command {}

impl Container for Command {
    fn lookup(&self, container_id: &str) -> Result<RawPid, Error> {
        let needle = container_id.as_bytes();
        let names = fsutil::read_dir_names("/proc").map_err(|source| Error::Io {
            path: PathBuf::from("/proc"),
            source,
        })?;
        let own_pid = std::process::id() as RawPid;

        for name in names {
            let cmdline = Path::new("/proc").join(&name).join("cmdline");
            let pid = match name.to_string_lossy().parse::<RawPid>() {
                Ok(pid) => pid,
                _ => {
                    continue;
                }
            };
            if pid == own_pid {
                continue;
            }

            // ignore error if process exits before we can read it
            if let Ok(mut arguments) = fsutil::read(&cmdline) {
                // treat all arguments as one large string
                for byte in arguments.iter_mut() {
                    if *byte == b'\0' {
                        *byte = b' ';
                    }
                }
                if arguments
                    .windows(needle.len())
                    .any(|window| window == needle)
                {
                    return Ok(pid);
                }
            }
        }

        Err(Error::ContainerNotFound {
            container: container_id.to_string(),
            message: "no process found with a matching command line".to_string(),
        })
    }
    fn check_required_tools(&self) -> Result<(), Error> {
        Ok(())
    }
}
