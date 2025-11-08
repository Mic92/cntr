use anyhow::Context;
use log::warn;
use nix::{self, unistd};
use std::collections::HashMap;
use std::env;
use std::ffi::OsString;
use std::fs::File;
use std::io;
use std::io::{BufRead, BufReader};
use std::os::unix::ffi::OsStringExt;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, ExitStatus};

use crate::paths;
use crate::procfs;
use crate::result::Result;

pub(crate) struct Cmd {
    environment: HashMap<OsString, OsString>,
    command: String,
    arguments: Vec<String>,
    home: Option<PathBuf>,
}

fn read_environment(pid: unistd::Pid) -> Result<HashMap<OsString, OsString>> {
    let path = procfs::get_path().join(pid.to_string()).join("environ");
    let f = File::open(&path)
        .with_context(|| format!("failed to open environment file {}", path.display()))?;
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
    pub(crate) fn new(
        command: Option<String>,
        args: Vec<String>,
        pid: unistd::Pid,
        home: Option<PathBuf>,
    ) -> Result<Cmd> {
        let arguments = if command.is_none() {
            vec![String::from("-l")]
        } else {
            args
        };

        let command =
            command.unwrap_or_else(|| env::var("SHELL").unwrap_or_else(|_| String::from("sh")));

        let variables = read_environment(pid)
            .context("could not inherit environment variables from container")?;
        Ok(Cmd {
            command,
            arguments,
            environment: variables,
            home,
        })
    }
    pub(crate) fn run(mut self) -> Result<ExitStatus> {
        let default_path =
            OsString::from("/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin");
        self.environment.insert(
            OsString::from("PATH"),
            env::var_os("PATH").unwrap_or(default_path),
        );

        if let Some(path) = self.home {
            self.environment
                .insert(OsString::from("HOME"), path.into_os_string());
        }

        let cmd = Command::new(&self.command)
            .args(&self.arguments)
            .envs(&self.environment)
            .status();
        cmd.with_context(|| format!("failed to run command: {}", self.command))
    }

    pub(crate) fn exec_chroot(self) -> Result<()> {
        let base_dir = paths::get_base_dir();
        let err = unsafe {
            Command::new(&self.command)
                .args(self.arguments)
                .envs(self.environment)
                .pre_exec(move || {
                    if let Err(e) = unistd::chroot(&base_dir) {
                        warn!("failed to chroot to {}: {}", base_dir.display(), e);
                        return Err(io::Error::from_raw_os_error(e as i32));
                    }

                    if let Err(e) = env::set_current_dir("/") {
                        warn!("failed to change directory to /");
                        return Err(e);
                    }

                    Ok(())
                })
                .exec()
        };
        Err(err).with_context(|| format!("failed to execute command: {}", self.command))?;
        Ok(())
    }
}
