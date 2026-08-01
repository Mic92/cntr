//! Process environment access through a single snapshot.
//!
//! All environment lookups go through this module so that only one place
//! depends on `std::env`. Later the snapshot can be fed directly from the
//! initial envp instead.

use std::os::unix::ffi::OsStringExt;
use std::sync::OnceLock;

static SNAPSHOT: OnceLock<Vec<(Vec<u8>, Vec<u8>)>> = OnceLock::new();

/// Take the environment snapshot. Called once at program start; when running
/// on origin instead of libc/std this will be fed from the initial envp.
pub fn init() {
    let _ = SNAPSHOT.set(
        std::env::vars_os()
            .map(|(key, value)| (key.into_vec(), value.into_vec()))
            .collect(),
    );
}

fn snapshot() -> &'static [(Vec<u8>, Vec<u8>)] {
    // Lazy fallback keeps unit tests working without an explicit init().
    SNAPSHOT.get_or_init(|| {
        std::env::vars_os()
            .map(|(key, value)| (key.into_vec(), value.into_vec()))
            .collect()
    })
}

/// All environment variables as raw bytes (for building envp).
pub(crate) fn vars() -> &'static [(Vec<u8>, Vec<u8>)] {
    snapshot()
}

/// Look up an environment variable. Returns None if it is unset or not UTF-8.
pub(crate) fn var(name: &str) -> Option<&'static str> {
    let value = snapshot()
        .iter()
        .find(|(key, _)| key == name.as_bytes())
        .map(|(_, value)| value)?;
    std::str::from_utf8(value).ok()
}

/// Split a PATH-style variable into its entries.
pub(crate) fn split_paths(value: &str) -> impl Iterator<Item = &str> {
    value.split(':').filter(|dir| !dir.is_empty())
}
