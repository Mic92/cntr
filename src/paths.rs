use std::env;
use std::path::PathBuf;

/// Environment variable to override the default cntr base directory
const CNTR_BASE_DIR_ENV: &str = "CNTR_BASE_DIR";

/// Default base directory for cntr operations
const DEFAULT_CNTR_BASE_DIR: &str = "/var/lib/cntr";

/// Get the cntr base directory path
///
/// This function checks the CNTR_BASE_DIR environment variable first.
/// If not set, it defaults to /var/lib/cntr.
///
/// This is primarily useful for tests that want to use a temporary directory
/// instead of the system /var/lib/cntr path.
pub fn get_base_dir() -> PathBuf {
    env::var(CNTR_BASE_DIR_ENV)
        .ok()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_CNTR_BASE_DIR))
}
