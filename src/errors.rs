//! Helpers for reporting errors to the user.

use alloc::string::{String, ToString};
use core::error::Error;
use core::fmt::Write;

/// Format an error together with its full source chain, e.g.
/// `failed to attach: failed to open pty master: Permission denied`.
pub(crate) fn format_chain(err: &dyn Error) -> String {
    let mut msg = err.to_string();
    let mut source = err.source();
    while let Some(cause) = source {
        let _ = write!(msg, ": {}", cause);
        source = cause.source();
    }
    msg
}
