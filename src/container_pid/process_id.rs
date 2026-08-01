use rustix::io::Errno;

use crate::container_pid::Container;
use crate::container_pid::Error;
use crate::container_pid::RawPid;
use crate::fsutil;
use crate::procfs::get_path;

#[derive(Clone, Debug)]
pub(crate) struct ProcessId {}

impl Container for ProcessId {
    fn lookup(&self, container_id: &str) -> Result<RawPid, Error> {
        let pid = container_id
            .parse::<RawPid>()
            .map_err(|source| Error::InvalidProcessId(container_id.to_string(), source))?;

        let proc_path = get_path().join(pid.to_string());
        match fsutil::metadata(&proc_path) {
            Err(e) => {
                if e == Errno::NOENT {
                    Err(Error::NoSuchProcess(pid))
                } else {
                    Err(Error::Io {
                        path: proc_path,
                        source: e,
                    })
                }
            }
            Ok(_) => Ok(pid),
        }
    }
    fn check_required_tools(&self) -> Result<(), Error> {
        Ok(())
    }
}
