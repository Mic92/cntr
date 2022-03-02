use log::warn;
use nix::{self, unistd};
use simple_error::try_with;
use std::collections::HashMap;
use std::env;
use std::ffi::{CStr, CString, OsStr, OsString};
use std::fs::File;
use std::io;
use std::io::{BufRead, BufReader};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::process::CommandExt;
use std::process::{Command, ExitStatus};

use crate::procfs;
use crate::result::Result;

pub struct Cmd {
    environment: HashMap<OsString, OsString>,
    command: String,
    arguments: Vec<String>,
    home: Option<CString>,
}

fn read_environment(pid: unistd::Pid) -> Result<HashMap<OsString, OsString>> {
    let path = procfs::get_path().join(pid.to_string()).join("environ");
    let f = try_with!(File::open(&path), "failed to open {}", path.display());
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

        let variables = try_with!(
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
        let default_path =
            OsString::from("/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin");
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
        Ok(try_with!(cmd, "failed to run `sh -l`"))
    }

    pub fn exec_chroot(self) -> Result<()> {
        let err = unsafe {
            Command::new(&self.command)
                .args(self.arguments)
                .envs(self.environment)
                .pre_exec(|| {
                    match unistd::chroot("/var/lib/cntr") {
                        Err(e) => {
                            warn!("failed to chroot to /var/lib/cntr: {}", e);
                            return Err(io::Error::from_raw_os_error(e as i32));
                        }
                        _ => {}
                    }

                    if let Err(e) = env::set_current_dir("/") {
                        warn!("failed to change directory to /");
                        return Err(e);
                    }

                    Ok(())
                })
                .exec()
        };
        try_with!(Err(err), "failed to execute `{}`", self.command);
        Ok(())
    }
}
