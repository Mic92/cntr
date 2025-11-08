use anyhow::Context;
use std::fs::File;
use std::io::BufReader;
use std::io::prelude::*;
use std::path::PathBuf;

use crate::result::Result;

pub fn read_open_sockets() -> Result<Vec<PathBuf>> {
    let file = File::open("/proc/net/unix")
        .context("failed to open /proc/net/unix to read open sockets")?;

    let mut paths = vec![];

    for line in BufReader::new(file).lines().skip(1) {
        let line = line.context("failed to read line from /proc/net/unix")?;
        let fields: Vec<&str> = line.splitn(7, ' ').collect();
        if fields.len() != 8 || fields[7].starts_with('@') {
            continue;
        }
        paths.push(PathBuf::from(fields[7]));
    }

    Ok(paths)
}
