use anyhow::{Context, bail};
use nix::unistd::Pid;
use std::fs::File;
use std::io::BufReader;
use std::io::prelude::*;

use crate::procfs;
use crate::result::Result;

//$ cat /proc/self/mounts
// tmpfs /proc/kcore tmpfs rw,context="system_u:object_r:container_file_t:s0:c125,c287",nosuid,mode=755 0 0
fn find_mount_options(p: Pid) -> Result<String> {
    let path = procfs::get_path().join(format!("{}/mounts", p));
    let f = File::open(&path)
        .with_context(|| format!("failed to open mount file {}", path.display()))?;
    let reader = BufReader::new(f);
    for line in reader.lines() {
        let line = line.with_context(|| format!("failed to read line from {}", path.display()))?;
        let line = line.trim();
        let mut tokens = line.split_terminator([' ', '\t']).filter(|s| s != &"");

        if let Some(mountpoint) = tokens.nth(1)
            && let Some(options) = tokens.nth(1)
            && mountpoint == "/"
        {
            return Ok(String::from(options));
        }
    }
    bail!("did not find / in {}", path.display())
}

pub fn parse_selinux_context(p: Pid) -> Result<String> {
    let options = find_mount_options(p)
        .context("failed to find mount options for / filesystem")?;
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
