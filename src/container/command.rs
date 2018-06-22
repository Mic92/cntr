use container::Container;
use libc::pid_t;
use nix::unistd::{Pid, getpid};
use std::fs;
use types::{Error, Result};

#[derive(Clone, Debug)]
pub struct Command {}

impl Container for Command {
    fn lookup(&self, container_id: &str) -> Result<Pid> {
        let needle = container_id.as_bytes();
        let dir = tryfmt!(fs::read_dir("/proc"), "failed to read /proc directory");
        let own_pid = getpid();

        for entry in dir {
            let entry = tryfmt!(entry, "error while reading /proc");
            let cmdline = entry.path().join("cmdline");
            let pid = match entry.file_name().to_string_lossy().parse::<pid_t>() {
                Ok(pid) => Pid::from_raw(pid),
                _ => {
                    continue;
                }
            };
            if pid == own_pid {
                continue;
            }

            // ignore error if process exits before we can read it
            if let Ok(mut arguments) = fs::read(cmdline.clone()) {
                // treat all arguments as one large string
                for byte in arguments.iter_mut() {
                    if *byte == b'\0' {
                        *byte = b' ';
                    }
                }
                if arguments
                    .windows(needle.len())
                    .position(|window| window == needle)
                    .is_some()
                {
                    return Ok(pid);
                }
            }
        }

        errfmt!(format!("No command found that matches {}", container_id))
    }
    fn check_required_tools(&self) -> Result<()> {
        return Ok(());
    }
}
