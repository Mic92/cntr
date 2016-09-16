use fs::Cntr;
use fuse;
use libc;
use namespace;
use std::io::{Write, Read};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};


const user: &'static namespace::Kind = &namespace::USER;

#[test]
fn test_mount() {
    let mut child = Command::new("unshare")
        .args(&["--user", "--mount", "--", "sh", "-c", "cat"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let pid = child.id() as libc::pid_t;

    // synchronize with cat
    let mut buf = [b't'];
    let mut stdin = child.stdin.unwrap();
    {
        stdin.write(&buf).unwrap();
        child.stdout.unwrap().read_exact(&mut buf).unwrap();

        let ns = user.open(pid).unwrap();
        ns.apply().unwrap();
    }

    let buf = PathBuf::from("mnt");
    unsafe {
        fuse::spawn_mount(Cntr::new(), &buf, &[]);
    }
}
