use nix::errno::Errno;
use simple_error::{bail, try_with};
use std::env;
use std::ffi::OsString;
use std::fs;
use std::os::unix::ffi::OsStringExt;
use std::path::{Path, PathBuf};

use crate::files::mkdir_p;
use crate::result::Result;

pub struct TempDir {
    name: Option<PathBuf>,
}
impl TempDir {
    pub fn path(&self) -> &Path {
        self.name.as_ref().unwrap()
    }
    pub fn into_path(mut self) -> PathBuf {
        self.name.take().unwrap()
    }
}

pub fn tempdir() -> Result<TempDir> {
    let mut template = env::temp_dir();
    if !template.exists() {
        template = PathBuf::from("/dev/shm");
        if !template.exists() {
            template = PathBuf::from("/tmp");
            if !template.exists() {
                try_with!(mkdir_p(&template), "mkdir failed");
            }
        }
    }
    template.push("cntr.XXXXXX");
    let mut bytes = template.into_os_string().into_vec();
    // null byte
    bytes.push(0);
    let res = unsafe { libc::mkdtemp(bytes.as_mut_ptr().cast()) };
    if res.is_null() {
        bail!("mkdtemp failed with {}", Errno::last());
    } else {
        // remove null byte
        bytes.pop();
        let name = PathBuf::from(OsString::from_vec(bytes));
        Ok(TempDir { name: Some(name) })
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        if let Some(ref p) = self.name {
            let _ = fs::remove_dir_all(p);
        }
    }
}
