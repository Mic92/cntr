use nix::unistd::Pid;
use std::fs::File;
use std::io::prelude::*;
use std::io::BufReader;

use crate::procfs;
use crate::types::{Error, Result};

//$ cat /proc/self/mounts
// tmpfs /proc/kcore tmpfs rw,context="system_u:object_r:container_file_t:s0:c125,c287",nosuid,mode=755 0 0
fn find_mount_options(p: Pid) -> Result<String> {
    let path = procfs::get_path().join(format!("{}/mounts", p));
    let f = tryfmt!(File::open(&path), "failed to open {}", path.display());
    let reader = BufReader::new(f);
    for line in reader.lines() {
        let line = tryfmt!(line, "failed to read {}", path.display());
        let line = line.trim();
        let mut tokens = line
            .split_terminator(|s: char| s == ' ' || s == '\t')
            .filter(|s| s != &"");

        if let Some(mountpoint) = tokens.nth(1) {
            if let Some(options) = tokens.nth(1) {
                if mountpoint == "/" {
                    return Ok(String::from(options));
                }
            }
        }
    }
    errfmt!(format!("did not find / in {}", path.display()))
}

pub fn parse_selinux_context(p: Pid) -> Result<String> {
    let options = tryfmt!(find_mount_options(p), "failed to parse mount options of /");
    let needle = "context=\"";
    if let Some(index) = options.find(needle) {
        if let Some(context) = options[(index + needle.len())..].splitn(2, '"').next() {
            return Ok(String::from(context));
        } else {
            return errfmt!(format!("missing quotes selinux context: {}", options));
        };
    }
    errfmt!(format!(
        "no selinux mount option found for / entry: {}",
        options
    ))
}
