use rustix::fs::{Access, access};
use std::env;
use std::path::{Path, PathBuf};

use crate::container_pid::Error;
use crate::spawn;

/// Run a container runtime CLI and return its stdout, mapping spawn and
/// non-zero exit failures to container_pid errors.
pub(crate) fn output(program: &str, args: &[&str]) -> Result<Vec<u8>, Error> {
    let command = format!("{} {}", program, args.join(" "));
    let output = spawn::run(program, args).map_err(|source| Error::CommandFailedToRun {
        command: command.clone(),
        source,
    })?;
    if !output.success() {
        return Err(Error::CommandFailed {
            command,
            status: output.status(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }
    Ok(output.stdout)
}

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
