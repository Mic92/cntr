use anyhow::Context;
use nix::unistd::Pid;
use std::fs::{File, OpenOptions};
use std::io::BufReader;
use std::io::ErrorKind;
use std::io::prelude::*;
use std::path::{Path, PathBuf};

use crate::mount_context;
use crate::procfs;
use crate::result::Result;

#[derive(PartialEq, Eq)]
enum LSMKind {
    AppArmor,
    SELinux,
}

impl LSMKind {
    pub(crate) fn profile_path(&self, pid: Option<Pid>) -> PathBuf {
        match *self {
            LSMKind::AppArmor => {
                let process = pid.map_or(String::from("self"), |p| p.to_string());
                procfs::get_path().join(process).join("attr/current")
            }
            LSMKind::SELinux => {
                let process = pid.map_or(String::from("thread-self"), |p| p.to_string());
                procfs::get_path().join(process).join("attr/exec")
            }
        }
    }
}

pub(crate) struct LSMProfile {
    label: String,
    kind: LSMKind,
    label_file: File,
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

fn is_selinux_enabled() -> Result<bool> {
    let file = File::open("/proc/filesystems").context("failed to open /proc/filesystems")?;
    let reader = BufReader::new(file);
    for line in reader.lines() {
        let l = line.context("failed to read line from /proc/filesystems")?;
        if l.contains("selinuxfs") {
            return Ok(true);
        }
    }
    Ok(false)
}

fn check_type() -> Result<Option<LSMKind>> {
    if is_apparmor_enabled().context("failed to check AppArmor availability")? {
        Ok(Some(LSMKind::AppArmor))
    } else if is_selinux_enabled().context("failed to check SELinux availability")? {
        Ok(Some(LSMKind::SELinux))
    } else {
        Ok(None)
    }
}

fn read_proclabel(path: &Path, kind: &LSMKind) -> Result<String> {
    let mut attr = String::new();
    let mut file = File::open(path)
        .with_context(|| format!("failed to open LSM profile file {}", path.display()))?;
    file.read_to_string(&mut attr)
        .with_context(|| format!("failed to read LSM profile from {}", path.display()))?;

    if *kind == LSMKind::AppArmor {
        let fields: Vec<&str> = attr.trim_end().splitn(2, ' ').collect();
        Ok(fields[0].to_owned())
    } else {
        Ok(attr)
    }
}

pub(crate) fn read_profile(pid: Pid) -> Result<Option<LSMProfile>> {
    let kind = check_type()?;

    if let Some(kind) = kind {
        let target_path = kind.profile_path(Some(pid));
        let target_label = read_proclabel(&target_path, &kind)
            .context("failed to get security label of target process")?;

        let own_path = kind.profile_path(None);
        let own_label =
            read_proclabel(&own_path, &kind).context("failed to get own security label")?;

        if target_label == own_label {
            // nothing to do
            return Ok(None);
        }

        let res = OpenOptions::new().write(true).open(&own_path);

        return Ok(Some(LSMProfile {
            kind,
            label: target_label,
            label_file: res.with_context(|| {
                format!("failed to open LSM profile file {}", own_path.display())
            })?,
        }));
    }
    Ok(None)
}

impl LSMProfile {
    pub(crate) fn inherit_profile(&mut self) -> Result<()> {
        let attr = match self.kind {
            LSMKind::AppArmor => format!("changeprofile {}", self.label),
            LSMKind::SELinux => self.label.clone(),
        };

        self.label_file
            .write_all(attr.as_bytes())
            .with_context(|| format!("failed to write '{}' to LSM profile", attr))?;
        Ok(())
    }

    pub(crate) fn mount_label(&self, pid: Pid) -> Result<Option<String>> {
        match self.kind {
            LSMKind::AppArmor => Ok(None),
            LSMKind::SELinux => {
                let context = mount_context::parse_selinux_context(pid)
                    .context("failed to parse SELinux mount options")?;
                Ok(Some(context))
            }
        }
    }
}
