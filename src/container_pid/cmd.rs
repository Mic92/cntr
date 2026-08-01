use rustix::fs::{Access, access};
use std::env;
use std::path::{Path, PathBuf};

pub(crate) fn which<P>(exe_name: P) -> Option<PathBuf>
where
    P: AsRef<Path>,
{
    env::var_os("PATH").and_then(|paths| {
        env::split_paths(&paths)
            .filter_map(|dir| {
                let full_path = dir.join(&exe_name);
                if access(&full_path, Access::EXEC_OK).is_ok() {
                    Some(full_path)
                } else {
                    None
                }
            })
            .next()
    })
}
