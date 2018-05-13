use std::fs::File;
use std::io::BufReader;
use std::io::prelude::*;
use std::path::PathBuf;
use types::{Error, Result};

#[allow(dead_code)]
pub fn read_open_sockets() -> Result<Vec<PathBuf>> {
    let file = tryfmt!(File::open("/proc/net/unix"), "cannot open /proc/net/unix");

    let mut paths = vec![];

    for line in BufReader::new(file).lines().skip(1) {
        let line = tryfmt!(line, "failed to read /proc/net/unix");
        let fields: Vec<&str> = line.splitn(7, ' ').collect();
        if fields.len() != 8 || fields[7].starts_with('@') {
            continue;
        }
        paths.push(PathBuf::from(fields[7]));
    }

    Ok(paths)
}
