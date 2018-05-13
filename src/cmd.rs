use libc;
use nix::{self, unistd};
use procfs;
use std::collections::HashMap;
use std::env;
use std::ffi::{CStr, CString, OsStr, OsString};
use std::fs::File;
use std::io;
use std::io::{BufRead, BufReader};
use std::os::unix::ffi::{OsStringExt, OsStrExt};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};
use types::{Error, Result};

pub struct Cmd {
    environment: HashMap<OsString, OsString>,
    command: String,
    arguments: Vec<String>,
    home: Option<CString>,
}

fn read_environment(pid: unistd::Pid) -> Result<HashMap<OsString, OsString>> {
    let path = procfs::get_path().join(pid.to_string()).join("environ");
    let f = tryfmt!(File::open(&path), "failed to open {}", path.display());
    let reader = BufReader::new(f);
    let res: HashMap<OsString, OsString> = reader
        .split(b'\0')
        .filter_map(|var| {
            let var = match var {
                Ok(var) => var,
                Err(_) => return None,
            };

            let tuple: Vec<&[u8]> = var.splitn(2, |b| *b == b'=').collect();
            if tuple.len() != 2 {
                return None;
            }
            Some((
                OsString::from_vec(Vec::from(tuple[0])),
                OsString::from_vec(Vec::from(tuple[1])),
            ))
        })
        .collect();
    Ok(res)
}

pub fn which<P>(exe_name: P) -> Option<PathBuf>
where
    P: AsRef<Path>,
{
    env::var_os("PATH").and_then(|paths| {
        env::split_paths(&paths)
            .filter_map(|dir| {
                let full_path = dir.join(&exe_name);
                let res = unistd::access(&full_path, unistd::AccessMode::X_OK);
                if res.is_ok() { Some(full_path) } else { None }
            })
            .next()
    })
}

impl Cmd {
    pub fn new(
        command: Option<String>,
        args: Vec<String>,
        pid: unistd::Pid,
        home: Option<&CStr>,
    ) -> Result<Cmd> {

        let arguments = if command.is_none() {
            vec![String::from("-l")]
        } else {
            args
        };

        let command =
            command.unwrap_or_else(|| env::var("SHELL").unwrap_or_else(|_| String::from("sh")));

        let variables = tryfmt!(
            read_environment(pid),
            "could not inherit environment variables of container"
        );
        Ok(Cmd {
            command,
            arguments,
            environment: variables,
            home: home.map(|h| h.to_owned()),
        })
    }
    pub fn run(mut self) -> Result<ExitStatus> {
        let default_path = OsString::from(
            "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
        );
        self.environment.insert(
            OsString::from("PATH"),
            env::var_os("PATH").unwrap_or(default_path),
        );

        if let Some(path) = self.home {
            self.environment.insert(
                OsString::from("HOME"),
                OsStr::from_bytes(path.to_bytes()).to_os_string(),
            );
        }

        let cmd = Command::new(self.command)
            .args(self.arguments)
            .envs(self.environment)
            .status();
        Ok(tryfmt!(cmd, "failed to run `sh -l`"))
    }

    pub fn exec_chroot(self) -> Result<()> {
        let err = Command::new(&self.command)
            .args(self.arguments)
            .envs(self.environment)
            .before_exec(|| {
                match unistd::chroot("/var/lib/cntr") {
                    Err(nix::Error::Sys(errno)) => {
                        warn!(
                            "failed to chroot to /var/lib/cntr: {}",
                            nix::Error::Sys(errno)
                        );
                        return Err(io::Error::from(errno));
                    }
                    Err(e) => {
                        warn!("failed to chroot to /var/lib/cntr: {}", e);
                        return Err(io::Error::from_raw_os_error(libc::EINVAL));
                    }
                    _ => {}
                }

                if let Err(e) = env::set_current_dir("/") {
                    warn!("failed to change directory to /");
                    return Err(e);
                }

                Ok(())
            })
            .exec();
        tryfmt!(Err(err), "failed to execute `{}`", self.command);
        Ok(())
    }
}
