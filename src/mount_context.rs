use nix::unistd::Pid;
use simple_error::{bail, try_with};
use std::fs::File;
use std::io::prelude::*;
use std::io::BufReader;

use crate::procfs;
use crate::result::Result;

//$ cat /proc/self/mounts
// tmpfs /proc/kcore tmpfs rw,context="system_u:object_r:container_file_t:s0:c125,c287",nosuid,mode=755 0 0
fn find_mount_options(p: Pid) -> Result<String> {
    let path = procfs::get_path().join(format!("{}/mounts", p));
    let f = try_with!(File::open(&path), "failed to open {}", path.display());
    let reader = BufReader::new(f);
    for line in reader.lines() {
        let line = try_with!(line, "failed to read {}", path.display());
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
    bail!("did not find / in {}", path.display())
}

pub fn parse_selinux_context(p: Pid) -> Result<String> {
    let options = try_with!(find_mount_options(p), "failed to parse mount options of /");
    let needle = "context=\"";
    if let Some(index) = options.find(needle) {
        if let Some(context) = options[(index + needle.len())..].split('"').next() {
            return Ok(String::from(context));
        } else {
            bail!("missing quotes selinux context: {}", options);
        };
    }
    bail!("no selinux mount option found for / entry: {}", options)
}
