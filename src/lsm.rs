use anyhow::Context;
use nix::unistd::Pid;
use std::fs::File;
use std::io::ErrorKind;
use std::io::prelude::*;
use std::path::PathBuf;

use crate::procfs;
use crate::result::Result;

// TODO add support for SELinux

pub(crate) struct LSMProfile {
    pub(crate) label: String,
    pub(crate) own_path: PathBuf,
}

fn is_apparmor_enabled() -> Result<bool> {
    let aa_path = "/sys/module/apparmor/parameters/enabled";
    match File::open(aa_path) {
        Ok(mut file) => {
            let mut contents = String::new();
            file.read_to_string(&mut contents)
                .with_context(|| format!("failed to read {}", aa_path))?;
            Ok(contents == "Y\n")
        }
        Err(err) => {
            if err.kind() != ErrorKind::NotFound {
                return Err(err).with_context(|| format!("failed to open {}", aa_path));
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

fn read_apparmor_label(path: &PathBuf) -> Result<String> {
    let mut attr = String::new();
    let mut file = File::open(path)
        .with_context(|| format!("failed to open AppArmor profile file {}", path.display()))?;
    file.read_to_string(&mut attr)
        .with_context(|| format!("failed to read AppArmor profile from {}", path.display()))?;

    // AppArmor format is "profile_name (mode)", extract just the profile name
    let fields: Vec<&str> = attr.trim_end().splitn(2, ' ').collect();
    Ok(fields[0].to_owned())
}

pub(crate) fn read_profile(pid: Pid) -> Result<Option<LSMProfile>> {
    if !is_apparmor_enabled().context("failed to check AppArmor availability")? {
        return Ok(None);
    }

    let target_path = apparmor_profile_path(Some(pid));
    let target_label = read_apparmor_label(&target_path)
        .context("failed to get AppArmor label of target process")?;

    let own_path = apparmor_profile_path(None);
    let own_label = read_apparmor_label(&own_path).context("failed to get own AppArmor label")?;

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
    pub(crate) fn inherit_profile(&mut self) -> Result<()> {
        // Open the file in the process that will write to it (not the parent)
        let mut file = File::options()
            .write(true)
            .open(&self.own_path)
            .with_context(|| {
                format!(
                    "failed to open AppArmor profile file {}",
                    self.own_path.display()
                )
            })?;

        let attr = format!("changeprofile {}", self.label);
        file.write_all(attr.as_bytes())
            .with_context(|| format!("failed to write '{}' to AppArmor profile", attr))?;

        Ok(())
    }
}
