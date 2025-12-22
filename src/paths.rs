use nix::unistd;
use std::env;
use std::path::PathBuf;

/// Environment variable to override the default cntr base directory
const CNTR_BASE_DIR_ENV: &str = "CNTR_BASE_DIR";

/// Default base directory for cntr operations (root user)
const DEFAULT_CNTR_BASE_DIR: &str = "/var/lib/cntr";

/// Get the cntr base directory path
///
/// This function checks the CNTR_BASE_DIR environment variable first.
/// If not set:
/// - For root: uses /var/lib/cntr
/// - For non-root: uses $XDG_RUNTIME_DIR/cntr or ~/.local/share/cntr
///
/// This allows unprivileged users to run cntr without root access.
pub fn get_base_dir() -> PathBuf {
    // Check environment variable first
    if let Ok(dir) = env::var(CNTR_BASE_DIR_ENV) {
        return PathBuf::from(dir);
    }

    // For root, use the system directory
    if unistd::geteuid().is_root() {
        return PathBuf::from(DEFAULT_CNTR_BASE_DIR);
    }

    // For non-root users, prefer XDG_RUNTIME_DIR (usually /run/user/<uid>)
    if let Ok(runtime_dir) = env::var("XDG_RUNTIME_DIR") {
        let mut path = PathBuf::from(runtime_dir);
        path.push("cntr");
        return path;
    }

    // Fall back to ~/.local/share/cntr
    if let Ok(home) = env::var("HOME") {
        let mut path = PathBuf::from(home);
        path.push(".local");
        path.push("share");
        path.push("cntr");
        return path;
    }

    // Should not happen in normal circumstances, but provide a sensible default
    PathBuf::from(DEFAULT_CNTR_BASE_DIR)
}
