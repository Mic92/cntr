use rustix::process::Pid;
use std::fs::File;
use std::io::ErrorKind;
use std::io::prelude::*;
use std::path::PathBuf;
use thiserror::Error;

use crate::ApparmorMode;
use crate::procfs;

// TODO add support for SELinux

#[derive(Debug, Error)]
pub(crate) enum LsmError {
    #[error("failed to read {path}")]
    ReadEnabled {
        path: &'static str,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to open AppArmor profile file {path}")]
    OpenProfile {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to read AppArmor profile from {path}")]
    ReadProfile {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to write '{attr}' to AppArmor profile")]
    WriteProfile {
        attr: String,
        #[source]
        source: std::io::Error,
    },
}

pub(crate) struct LSMProfile {
    pub(crate) label: String,
    pub(crate) own_path: PathBuf,
}

fn is_apparmor_enabled() -> Result<bool, LsmError> {
    let aa_path = "/sys/module/apparmor/parameters/enabled";
    match File::open(aa_path) {
        Ok(mut file) => {
            let mut contents = String::new();
            file.read_to_string(&mut contents)
                .map_err(|source| LsmError::ReadEnabled {
                    path: aa_path,
                    source,
                })?;
            Ok(contents == "Y\n")
        }
        Err(err) => {
            if err.kind() != ErrorKind::NotFound {
                return Err(LsmError::ReadEnabled {
                    path: aa_path,
                    source: err,
                });
            }
            Ok(false)
        }
    }
}

fn apparmor_profile_path(pid: Option<Pid>) -> PathBuf {
    let process = pid.map_or(String::from("self"), |p| p.to_string());
    procfs::get_path()
        .join(process)
        .join("attr/apparmor/current")
}

fn read_apparmor_label(path: &PathBuf) -> Result<String, LsmError> {
    let mut attr = String::new();
    let mut file = File::open(path).map_err(|source| LsmError::OpenProfile {
        path: path.clone(),
        source,
    })?;
    file.read_to_string(&mut attr)
        .map_err(|source| LsmError::ReadProfile {
            path: path.clone(),
            source,
        })?;

    // AppArmor format is "profile_name (mode)", extract just the profile name
    let fields: Vec<&str> = attr.trim_end().splitn(2, ' ').collect();
    Ok(fields[0].to_owned())
}

pub(crate) fn read_profile(
    pid: Pid,
    apparmor_mode: ApparmorMode,
) -> Result<Option<LSMProfile>, LsmError> {
    // If AppArmor is disabled via flag, return None
    if apparmor_mode == ApparmorMode::Off {
        return Ok(None);
    }

    if !is_apparmor_enabled()? {
        return Ok(None);
    }

    let target_path = apparmor_profile_path(Some(pid));
    let target_label = read_apparmor_label(&target_path)?;

    let own_path = apparmor_profile_path(None);
    let own_label = read_apparmor_label(&own_path)?;

    if target_label == own_label {
        // Already have the same profile, nothing to do
        return Ok(None);
    }

    // Don't open the file here - it must be opened by the same process that writes to it
    Ok(Some(LSMProfile {
        label: target_label,
        own_path,
    }))
}

impl LSMProfile {
    pub(crate) fn inherit_profile(&mut self) -> Result<(), LsmError> {
        // Open the file in the process that will write to it (not the parent)
        let mut file = File::options()
            .write(true)
            .open(&self.own_path)
            .map_err(|source| LsmError::OpenProfile {
                path: self.own_path.clone(),
                source,
            })?;

        let attr = format!("changeprofile {}", self.label);
        file.write_all(attr.as_bytes())
            .map_err(|source| LsmError::WriteProfile { attr, source })?;

        Ok(())
    }
}
