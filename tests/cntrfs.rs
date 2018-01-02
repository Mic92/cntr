extern crate fuse;
extern crate libc;
extern crate cntr;
extern crate nix;

#[cfg(feature = "profiling")]
extern crate cpuprofiler;

extern crate parking_lot;

use cntr::fs::{CntrFs, CntrMountOptions};

#[cfg(feature = "profiling")]
use cpuprofiler::PROFILER;
use nix::unistd;
use std::env;
use std::io::Write;
use std::path::Path;
use std::process;

fn main() {
    if cfg!(feature = "verbose_fuse_test_log") {
        cntr::enable_debug_log().unwrap();
    }

    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        println!("USAGE: {} from_path to_path", args[0]);
        process::exit(1);
    }

    if let unistd::ForkResult::Parent { .. } = unistd::fork().unwrap() {
        return;
    }

    if cfg!(feature = "splice_read") {
        println!("enable splice read");
    }
    if cfg!(feature = "splice_write") {
        println!("enable splice write");
    }
    #[cfg(feature = "profiling")] PROFILER.lock().unwrap().start("./cntrfs.profile").unwrap();

    let fs = CntrFs::new(&CntrMountOptions {
        prefix: &args[1],
        splice_read: cfg!(feature = "splice_read"),
        splice_write: cfg!(feature = "splice_write"),
        uid_map: cntr::DEFAULT_ID_MAP,
        gid_map: cntr::DEFAULT_ID_MAP,
    });

    match fs {
        Ok(cntr) => {
            cntr.mount(Path::new(&args[2])).unwrap();
        }
        Err(err) => {
            let _ = writeln!(&mut std::io::stderr(), "{}", err);
            process::exit(1);
        }
    };
    #[cfg(feature = "profiling")] PROFILER.lock().unwrap().stop().unwrap();

    //let output = Command::new("xfstests-check")
    //    .arg("-overlay")
    //    .env("TEST_DIR", "./tests/dest-mnt")
    //    .env("TEST_DEV", "./tests/dest-src")
    //    .spawn()
    //    .unwrap();

    //fs::read_dir("from/abc").unwrap();
}
