//! Logging and user-facing output on top of rustix' stderr.
//!
//! Keeps all terminal output in one place so that neither the `log` macros
//! nor error reporting need std's stdio machinery.

use alloc::string::ToString;
use log::{LevelFilter, Log, Metadata, Record};

/// Write a formatted line to stderr, ignoring errors (like eprintln!).
pub fn write_stderr(args: core::fmt::Arguments) {
    let mut line = args.to_string();
    line.push('\n');
    let _ = crate::fsutil::write_all(unsafe { rustix::stdio::stderr() }, line.as_bytes());
}

/// eprintln! replacement that writes through rustix instead of std stdio.
#[macro_export]
macro_rules! stderrln {
    () => { $crate::logging::write_stderr(format_args!("")) };
    ($($arg:tt)*) => { $crate::logging::write_stderr(format_args!($($arg)*)) };
}

struct StderrLogger;

impl Log for StderrLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= log::max_level()
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            write_stderr(format_args!("[{}] {}", record.level(), record.args()));
        }
    }

    fn flush(&self) {}
}

/// Install the stderr logger. The level defaults to warnings and can be
/// raised with `CNTR_LOG` (error, warn, info, debug, trace).
pub(crate) fn init() {
    let level = match crate::env::var("CNTR_LOG") {
        Some("error") => LevelFilter::Error,
        Some("info") => LevelFilter::Info,
        Some("debug") => LevelFilter::Debug,
        Some("trace") => LevelFilter::Trace,
        _ => LevelFilter::Warn,
    };
    if log::set_logger(&StderrLogger).is_ok() {
        log::set_max_level(level);
    }
}
