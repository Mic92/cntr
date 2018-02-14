use nix::unistd;
use std::collections::HashMap;
use std::env;
use std::ffi::{CStr, OsStr, OsString};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::os::unix::ffi::{OsStringExt, OsStrExt};
use std::path::PathBuf;
use std::process::{Command, ExitStatus};
use types::{Error, Result};

pub struct Cmd {
    environment: HashMap<OsString, OsString>,
}

fn read_environment(pid: unistd::Pid) -> Result<HashMap<OsString, OsString>> {
    let mut buf = PathBuf::from("/proc/");
    buf.push(pid.to_string());
    buf.push("environ");
    let path = buf.as_path();
    let f = tryfmt!(
        File::open(path),
        "failed to open {}",
        path.to_str().unwrap()
    );
    let reader = BufReader::new(f);
    let res: HashMap<OsString, OsString> = reader
        .split(b'\0')
        .filter_map(|var| {
            let var = match var {
                Ok(var) => var,
                Err(_) => return None,
            };

            let tuple: Vec<&[u8]> = var.splitn(1, |b| *b == b'=').collect();
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
    pub fn new(pid: unistd::Pid, home: Option<&CStr>) -> Result<Cmd> {
        let mut variables = tryfmt!(
            read_environment(pid),
            "could not inherit environment variables of container"
        );
        let default_path = OsString::from(
            "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
        );
        variables.insert(
            OsString::from("PATH"),
            env::var_os("PATH").unwrap_or(default_path),
        );
        if let Some(path) = home {
            variables.insert(
                OsString::from("HOME"),
                OsStr::from_bytes(path.to_bytes()).to_os_string(),
            );
        }
        Ok(Cmd { environment: variables })
    }
    pub fn run(self) -> Result<ExitStatus> {
        let shell = env::var("SHELL").unwrap_or(String::from("sh"));
        let cmd = Command::new(shell)
            .args(&["-l"])
            .envs(self.environment)
            .status();
        Ok(tryfmt!(cmd, "failed to run `sh -l`"))
    }
}
