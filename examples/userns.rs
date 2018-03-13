extern crate fuse;
extern crate libc;
extern crate cntr;
extern crate log;
extern crate nix;


use cntr::namespace;
use nix::unistd;
use procfs;
use std::fs::read_link;
use std::io::{Write, Read};
use std::process::{self, Command, Stdio};

const USER: &namespace::Kind = &namespace::USER;
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
    stdin.write_all(&buf).unwrap();
    stdout.read_exact(&mut buf).unwrap();
    assert_eq!(buf, [b't']);


    println!(
        "{} -> {}",
        read_link("/proc/self/ns/user").unwrap().display(),
        read_link(procfs::get_path().join(format!("{}/ns/user", pid)))
            .unwrap()
            .display()
    );
    USER.open(pid).unwrap().apply().unwrap();
    (stdin, stdout)
}

fn main() {
    testname_space();
}
