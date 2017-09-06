extern crate fuse;
extern crate libc;
extern crate cntr;
extern crate log;
extern crate nix;

use nix::unistd;
use std::fs::read_link;
use std::process::{Command, Stdio, self};
use std::io::{Write, Read};

use cntr::namespace;

const USER : &'static namespace::Kind = &namespace::USER;
fn testname_space() -> (process::ChildStdin, process::ChildStdout) {
    let child = Command::new("unshare")
        .args(&["--user", "--mount", "--", "sh", "-c", "cat"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let pid = unistd::Pid::from_raw(child.id() as libc::pid_t);

    // synchronize with cat
    let mut buf = [b't'];
    let mut stdin = child.stdin.unwrap();
    let mut stdout = child.stdout.unwrap();
    stdin.write(&buf).unwrap();
    stdout.read_exact(&mut buf).unwrap();
    assert_eq!(buf, [b't']);

    println!("{} -> {}", read_link(format!("/proc/self/ns/user")).unwrap().display(),
             read_link(format!("/proc/{}/ns/user", pid)).unwrap().display());
    USER.open(pid).unwrap().apply().unwrap();
    return (stdin, stdout);
}

fn main() {
    testname_space();
}
