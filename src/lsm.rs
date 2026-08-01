use rustix::io::Errno;
use rustix::process::Pid;
use thiserror::Error;
use typed_path::UnixPathBuf;

use crate::ApparmorMode;
use crate::fsutil;
use crate::procfs;

// TODO add support for SELinux

#[derive(Debug, Error)]
pub(crate) enum LsmError {
    #[error("failed to read {path}")]
    ReadEnabled {
        path: &'static str,
        #[source]
        source: Errno,
    },
    #[error("failed to read AppArmor profile from {}", path.display())]
    ReadProfile {
        path: UnixPathBuf,
        #[source]
        source: Errno,
    },
    #[error("failed to write '{attr}' to AppArmor profile")]
    WriteProfile {
        attr: String,
        #[source]
        source: Errno,
    },
}

pub(crate) struct LSMProfile {
    pub(crate) label: String,
    pub(crate) own_path: UnixPathBuf,
}

fn is_apparmor_enabled() -> Result<bool, LsmError> {
    let aa_path = "/sys/module/apparmor/parameters/enabled";
    match fsutil::read_to_string(aa_path) {
        Ok(contents) => Ok(contents == "Y\n"),
        Err(Errno::NOENT) => Ok(false),
        Err(source) => Err(LsmError::ReadEnabled {
            path: aa_path,
            source,
        }),
    }
}

fn apparmor_profile_path(pid: Option<Pid>) -> UnixPathBuf {
    let process = pid.map_or(String::from("self"), |p| p.to_string());
    procfs::get_path()
        .join(process)
        .join("attr/apparmor/current")
}

fn read_apparmor_label(path: &UnixPathBuf) -> Result<String, LsmError> {
    let attr = fsutil::read_to_string(path).map_err(|source| LsmError::ReadProfile {
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
        // The profile file must be written by the same process that keeps it,
        // so this runs in the attaching process, not the parent.
        let attr = format!("changeprofile {}", self.label);
        fsutil::write_existing(&self.own_path, &attr)
            .map_err(|source| LsmError::WriteProfile { attr, source })?;

        Ok(())
    }
}
