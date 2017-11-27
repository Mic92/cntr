extern crate fuse;
extern crate libc;
extern crate cntr;
extern crate log;
extern crate nix;

#[cfg(feature="profiling")]
extern crate cpuprofiler;

use cntr::fs::CntrFs;
use nix::unistd;
use std::env;
use std::io::Write;
use std::path::Path;
use std::process;

#[cfg(feature="profiling")]
use cpuprofiler::PROFILER;

struct Logger;
impl log::Log for Logger {
    fn enabled(&self, _: &log::LogMetadata) -> bool {
        true
    }
    fn log(&self, record: &log::LogRecord) {
        println!("{} - {}", record.level(), record.args());
    }
}

fn main() {
    if cfg!(feature = "testlogging") {
        log::set_logger(|max_log_level| {
            max_log_level.set(log::LogLevelFilter::Debug);
            Box::new(Logger)
        }).unwrap();
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
    #[cfg(feature = "profiling")]
    PROFILER.lock().unwrap().start("./cntrfs.profile").unwrap();

    match CntrFs::new(&args[1], cfg!(feature="splice_read")) {
        Ok(cntr) => {
            cntr.mount(Path::new(&args[2]), cfg!(feature="splice_write")).unwrap();
        },
        Err(err) => {
            let _ = writeln!(&mut std::io::stderr(), "{}", err);
            process::exit(1);
        }
    };
    #[cfg(feature = "profiling")]
    PROFILER.lock().unwrap().stop().unwrap();

    //let output = Command::new("xfstests-check")
    //    .arg("-overlay")
    //    .env("TEST_DIR", "./tests/dest-mnt")
    //    .env("TEST_DEV", "./tests/dest-src")
    //    .spawn()
    //    .unwrap();

    //fs::read_dir("from/abc").unwrap();
}
