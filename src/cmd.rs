use namespace;
use nix::unistd;
use std::env;
use std::ffi::{CString, OsStr};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;
use types::{Error, Result};
use std::process::{Command,ExitStatus};
use void;

fn read_environ(pid: unistd::Pid) -> Result<Vec<CString>> {
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
    reader
        .split(b'\0')
        .map(|var| {
            let r = tryfmt!(var, "failed to read");
            Ok(CString::new(r).unwrap())
        })
        .collect()
}

fn inherit_path(pid: unistd::Pid) -> Result<()> {
    let env = tryfmt!(
        read_environ(pid),
        "failed to get environment variables of target process {}",
        pid
    );

    let default_path = "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";
    let path = match env.iter().find(|var| var.as_bytes().starts_with(b"PATH=")) {
        Some(n) => &n.as_bytes()[5..],
        None => default_path.as_bytes(),
    };
    env::set_var("PATH", OsStr::from_bytes(&path));
    Ok(())
}

pub fn exec() -> Result<ExitStatus> {
    Ok(tryfmt!(Command::new("/bin/sh").args(&["-l"]).status(), "failed to run `/bin/sh -l`"))
}
