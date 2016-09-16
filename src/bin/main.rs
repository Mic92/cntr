extern crate argparse;
extern crate cntr;

use std::io::Write;
use argparse::{ArgumentParser, Store};
use std::process;

fn parse_args() -> cntr::Options {
    let mut options = cntr::Options { pid: 0, mountpoint: "/".to_string() };
    {
        let mut ap = ArgumentParser::new();
        ap.set_description("Enter container");
        ap.refer(&mut options.pid)
          .add_argument("pid", Store, "target pid");
        ap.refer(&mut options.mountpoint)
          .add_argument("mountpoint", Store, "fuse mountpoint");
        ap.parse_args_or_exit();
    }
    return options;
}

fn main() {
    let opts = parse_args();
    match cntr::run(opts) {
        Err(err) => {
            let _ = writeln!(&mut std::io::stderr(), "{}", err);
            process::exit(1);
        }
        _ => {}
    };
}
