//! Process environment access through a single snapshot.
//!
//! All environment lookups go through this module so that only one place
//! knows where the environment comes from. The binary feeds it from
//! std::env today and from origin's envp once the crate runs without libc.

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::ptr;
use core::sync::atomic::{AtomicPtr, Ordering};

static SNAPSHOT: AtomicPtr<Vec<(Vec<u8>, Vec<u8>)>> = AtomicPtr::new(ptr::null_mut());

/// Install the environment snapshot. Called once at program start.
pub fn init(environ: Vec<(Vec<u8>, Vec<u8>)>) {
    let leaked = Box::into_raw(Box::new(environ));
    if SNAPSHOT
        .compare_exchange(ptr::null_mut(), leaked, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        // Already initialized: drop the new snapshot and keep the old one.
        drop(unsafe { Box::from_raw(leaked) });
    }
}

fn snapshot() -> &'static [(Vec<u8>, Vec<u8>)] {
    let ptr = SNAPSHOT.load(Ordering::Acquire);
    if ptr.is_null() {
        // Lazy fallback keeps unit tests working without an explicit init().
        #[cfg(any(test, feature = "std"))]
        {
            use std::os::unix::ffi::OsStringExt;
            init(
                std::env::vars_os()
                    .map(|(key, value)| (key.into_vec(), value.into_vec()))
                    .collect(),
            );
            return snapshot();
        }
        #[cfg(not(any(test, feature = "std")))]
        return &[];
    }
    unsafe { &*ptr }
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
    core::str::from_utf8(value).ok()
}

/// Split a PATH-style variable into its entries.
pub(crate) fn split_paths(value: &str) -> impl Iterator<Item = &str> {
    value.split(':').filter(|dir| !dir.is_empty())
}
