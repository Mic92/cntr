//! User lookups via libc's getpwnam_r (NSS-aware).

use rustix::process::{Gid, Uid};
use std::ffi::{CStr, CString};
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub(crate) enum PasswdError {
    #[error("user name '{0}' contains a NUL byte")]
    InvalidName(String),
    #[error("getpwnam_r failed for user '{user}'")]
    Getpwnam {
        user: String,
        #[source]
        source: std::io::Error,
    },
}

/// A user entry from the passwd database
#[derive(Debug, Clone)]
pub(crate) struct User {
    pub(crate) uid: Uid,
    pub(crate) gid: Gid,
    /// Home directory
    pub(crate) dir: PathBuf,
}

/// Look up a user by name.
///
/// Returns Ok(None) if the user does not exist.
pub(crate) fn lookup(name: &str) -> Result<Option<User>, PasswdError> {
    let c_name = CString::new(name).map_err(|_| PasswdError::InvalidName(name.to_string()))?;
    let mut pwd: libc::passwd = unsafe { std::mem::zeroed() };
    let mut buf = vec![0u8; 4096];
    let mut result: *mut libc::passwd = std::ptr::null_mut();

    loop {
        // SAFETY: all pointers reference live buffers for the duration of the call.
        let err = unsafe {
            libc::getpwnam_r(
                c_name.as_ptr(),
                &mut pwd,
                buf.as_mut_ptr().cast(),
                buf.len(),
                &mut result,
            )
        };
        if err == libc::ERANGE {
            // Entry does not fit; retry with a larger buffer.
            buf.resize(buf.len() * 2, 0);
            continue;
        }
        if err != 0 {
            return Err(PasswdError::Getpwnam {
                user: name.to_string(),
                source: std::io::Error::from_raw_os_error(err),
            });
        }
        if result.is_null() {
            return Ok(None);
        }
        // SAFETY: result points to pwd, filled in by getpwnam_r; pw_dir is a
        // NUL-terminated string within buf.
        let dir = unsafe { CStr::from_ptr(pwd.pw_dir) };
        return Ok(Some(User {
            uid: Uid::from_raw(pwd.pw_uid),
            gid: Gid::from_raw(pwd.pw_gid),
            dir: PathBuf::from(String::from_utf8_lossy(dir.to_bytes()).into_owned()),
        }));
    }
}
