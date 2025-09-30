use nix::unistd::Pid;
use simple_error::try_with;
use std::fs::{File, OpenOptions};
use std::io::prelude::*;
use std::io::BufReader;
use std::io::ErrorKind;
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
    pub fn profile_path(&self, pid: Option<Pid>) -> PathBuf {
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

pub struct LSMProfile {
    label: String,
    kind: LSMKind,
    label_file: File,
}

fn is_apparmor_enabled() -> Result<bool> {
    let aa_path = "/sys/module/apparmor/parameters/enabled";
    match File::open(aa_path) {
        Ok(mut file) => {
            let mut contents = String::new();
            try_with!(
                file.read_to_string(&mut contents),
                "failed to read {}",
                aa_path
            );
            Ok(contents == "Y\n")
        }
        Err(err) => {
            if err.kind() != ErrorKind::NotFound {
                try_with!(Err(err), "failed to open {}", aa_path);
            }
            Ok(false)
        }
    }
}

fn is_selinux_enabled() -> Result<bool> {
    let file = try_with!(
        File::open("/proc/filesystems"),
        "failed to open /proc/filesystems"
    );
    let reader = BufReader::new(file);
    for line in reader.lines() {
        let l = try_with!(line, "failed to read from /proc/filesystems");
        if l.contains("selinuxfs") {
            return Ok(true);
        }
    }
    Ok(false)
}

fn check_type() -> Result<Option<LSMKind>> {
    if try_with!(
        is_apparmor_enabled(),
        "failed to check availability of apparmor"
    ) {
        Ok(Some(LSMKind::AppArmor))
    } else if try_with!(
        is_selinux_enabled(),
        "failed to check availability of selinux"
    ) {
        Ok(Some(LSMKind::SELinux))
    } else {
        Ok(None)
    }
}

fn read_proclabel(path: &Path, kind: &LSMKind) -> Result<String> {
    let mut attr = String::new();
    let mut file = try_with!(File::open(path), "failed to open {}", path.display());
    try_with!(
        file.read_to_string(&mut attr),
        "failed to read {}",
        path.display()
    );

    if *kind == LSMKind::AppArmor {
        let fields: Vec<&str> = attr.trim_end().splitn(2, ' ').collect();
        Ok(fields[0].to_owned())
    } else {
        Ok(attr)
    }
}

pub fn read_profile(pid: Pid) -> Result<Option<LSMProfile>> {
    let kind = check_type()?;

    if let Some(kind) = kind {
        let target_path = kind.profile_path(Some(pid));
        let target_label = try_with!(
            read_proclabel(&target_path, &kind),
            "failed to get security label of target process"
        );

        let own_path = kind.profile_path(None);
        let own_label = try_with!(
            read_proclabel(&own_path, &kind),
            "failed to get own security label"
        );

        if target_label == own_label {
            // nothing to do
            return Ok(None);
        }

        let res = OpenOptions::new().write(true).open(&own_path);

        return Ok(Some(LSMProfile {
            kind,
            label: target_label,
            label_file: try_with!(res, "failed to open {}", own_path.display()),
        }));
    }
    Ok(None)
}

impl LSMProfile {
    pub fn inherit_profile(mut self) -> Result<()> {
        let attr = match self.kind {
            LSMKind::AppArmor => format!("changeprofile {}", self.label),
            LSMKind::SELinux => self.label,
        };

        let res = self.label_file.write_all(attr.as_bytes());
        try_with!(res, "failed to write '{}' to /proc/self/attr/current", attr);
        Ok(())
    }

    pub fn mount_label(&self, pid: Pid) -> Result<Option<String>> {
        match self.kind {
            LSMKind::AppArmor => Ok(None),
            LSMKind::SELinux => {
                let context = try_with!(
                    mount_context::parse_selinux_context(pid),
                    "failed to parse selinux mount options"
                );
                Ok(Some(context))
            }
        }
    }
}
