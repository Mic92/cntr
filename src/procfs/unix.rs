use simple_error::try_with;
use std::fs::File;
use std::io::prelude::*;
use std::io::BufReader;
use std::path::PathBuf;

use crate::result::Result;

#[allow(dead_code)]
pub fn read_open_sockets() -> Result<Vec<PathBuf>> {
    let file = try_with!(File::open("/proc/net/unix"), "cannot open /proc/net/unix");

    let mut paths = vec![];

    for line in BufReader::new(file).lines().skip(1) {
        let line = try_with!(line, "failed to read /proc/net/unix");
        let fields: Vec<&str> = line.splitn(7, ' ').collect();
        if fields.len() != 8 || fields[7].starts_with('@') {
            continue;
        }
        paths.push(PathBuf::from(fields[7]));
    }

    Ok(paths)
}
