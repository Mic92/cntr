use libc;
use nix::unistd;
use std::ffi::{CString, OsStr};
use std::path::PathBuf;
use std::fs::File;
use types::{Error, Result};
use std::io::{BufRead, BufReader};
use std::os::unix::ffi::OsStrExt;
use std::env;
use namespace;
use trace;

fn read_environ(pid: libc::pid_t) -> Result<Vec<CString>> {
    let mut buf = PathBuf::from("/proc/");
    buf.push(pid.to_string());
    buf.push("environ");
    let path = buf.as_path();
    let f = tryfmt!(File::open(path),
                    "failed to open {}",
                    path.to_str().unwrap());
    let reader = BufReader::new(f);
    reader.split(b'\0')
          .map(|var| {
              let r = tryfmt!(var, "failed to read");
              Ok(CString::new(r).unwrap())
          })
          .collect()
}

fn setns(pid: libc::pid_t) -> Result<()> {
    let supported = tryfmt!(namespace::supported_namespaces(),
                            "can not get supported namespaces");
    let mut namespaces = Vec::new();
    for kind in supported {
        if !kind.is_same(pid) {
            namespaces.push(tryfmt!(kind.open(pid), "failed to open namespace"));
        }
    }
    for ns in namespaces {
        let name = &ns.kind.name;
        tryfmt!(ns.apply(), "failed to apply {} namespace", name);
    }
    Ok(())
}

fn inherit_path(pid: libc::pid_t) -> Result<()> {
    let env = tryfmt!(read_environ(pid),
                      "failed to get environment variables of target process {}",
                      pid);

    let default_path = "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";
    let path = match env.iter().find(|var| var.as_bytes().starts_with(b"PATH=")) {
        Some(n) => &n.as_bytes()[5..],
        None => default_path.as_bytes(),
    };
    env::set_var("PATH", OsStr::from_bytes(&path));
    Ok(())
}

pub fn exec(pid: libc::pid_t) -> Result<()> {
    let arg0 = CString::new("/bin/sh").unwrap();
    let arg1 = CString::new("-l").unwrap();
    tryfmt!(setns(pid), "failed to enter namespace");

    tryfmt!(trace::me(), "ptrace(PTRACE_TRACME) failed");
    // Ok(tryfmt!(unistd::execvpe(&arg0, &[arg0.clone(), arg1], env.as_slice()),
    //           "failed to execute shell"))
    Ok(tryfmt!(unistd::execvp(&arg0, &[arg0.clone(), arg1]),
               "failed to execute shell"))
}
