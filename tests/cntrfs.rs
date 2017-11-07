extern crate fuse;
extern crate libc;
extern crate cntr;
extern crate log;
extern crate nix;
extern crate cpuprofiler;

use cntr::fs::CntrFs;
use nix::unistd;
use std::env;
use std::io::Write;
use std::path::Path;
use std::process;
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

const SPLICE_READ: bool = false;
const SPLICE_WRITE: bool = false;
const ENABLE_PROFILING: bool = false;

fn main() {
    //let _ = log::set_logger(|max_log_level| {
    //    max_log_level.set(log::LogLevelFilter::Debug);
    //    Box::new(Logger)
    //});

    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        println!("USAGE: {} from_path to_path", args[0]);
        process::exit(1);
    }

    if let unistd::ForkResult::Parent { .. } = unistd::fork().unwrap() {
        return;
    }

    if SPLICE_READ {
        println!("enable splice read");
    }
    if SPLICE_WRITE {
        println!("enable splice write");
    }
    match CntrFs::new(&args[1], SPLICE_READ) {
        Ok(cntr) => {
            if ENABLE_PROFILING {
                PROFILER.lock().unwrap().start("./cntrfs.profile").unwrap();
            }
            cntr.mount(Path::new(&args[2]), SPLICE_WRITE).unwrap();
            if ENABLE_PROFILING {
                PROFILER.lock().unwrap().stop().unwrap();
            }
        },
        Err(err) => {
            let _ = writeln!(&mut std::io::stderr(), "{}", err);
            process::exit(1);
        }
    };

    //let output = Command::new("xfstests-check")
    //    .arg("-overlay")
    //    .env("TEST_DIR", "./tests/dest-mnt")
    //    .env("TEST_DEV", "./tests/dest-src")
    //    .spawn()
    //    .unwrap();

    //fs::read_dir("from/abc").unwrap();
}
