use log;

struct Logger;

impl log::Log for Logger {
    fn enabled(&self, _metadata: &log::LogMetadata) -> bool {
        true
    }

    fn log(&self, record: &log::LogRecord) {
        if self.enabled(record.metadata()) {
            eprintln!("{} - {}", record.level(), record.args());
        }
    }
}

pub fn enable_debug_log() -> Result<(), log::SetLoggerError> {
    log::set_logger(|max_log_level| {
        max_log_level.set(log::LogLevelFilter::Debug);
        Box::new(Logger)
    })
}
