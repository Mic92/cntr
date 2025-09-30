use libc::pid_t;
use nix::fcntl::{self, OFlag};
use nix::sys::stat;
use nix::unistd::Pid;
use simple_error::try_with;
use std::fs::{self, File};
use std::io::Write;
use std::os::unix::prelude::*;
use std::{
    fs::{set_permissions, Permissions},
    os::unix::fs::PermissionsExt,
};

use std::fs::OpenOptions;
use std::os::unix::fs::OpenOptionsExt;

use crate::capabilities;
use crate::procfs::ProcStatus;
use crate::result::Result;
use crate::tmp;

/// Hidden directory with CAP_CHROOT enabled cntr-exec binary
pub struct DotcntrDir {
    pub file: File,
    pub dir: tmp::TempDir,
}

impl DotcntrDir {
    pub fn write_pid_file(&self, target_pid: Pid) -> Result<()> {
        let path = self.dir.path().join("pid");
        let mut file = try_with!(
            OpenOptions::new()
                .create_new(true)
                .mode(0o644)
                .write(true)
                .open(&path),
            "failed to create {}",
            path.display()
        );

        let raw_pid: pid_t = target_pid.into();
        try_with!(
            file.write_all(format!("{}", raw_pid).as_bytes()),
            "failed to write {}",
            path.display()
        );
        Ok(())
    }

    pub fn write_setcap_exe(&self) -> Result<()> {
        let path = self.dir.path().join("cntr-exec");
        try_with!(
            fs::copy("/proc/self/exe", &path),
            "failed to copy /proc/self/exe to {}",
            path.display()
        );

        try_with!(
            capabilities::set_chroot_capability(&path),
            "Failed set file capability CAP_SYS_CHROOT on {}",
            path.display()
        );
        Ok(())
    }
}

pub fn create(process_status: &ProcStatus) -> Result<DotcntrDir> {
    let dotcntr_dir = try_with!(tmp::tempdir(), "failed to create temporary directory");
    let permissions = Permissions::from_mode(0o755);
    try_with!(
        set_permissions(dotcntr_dir.path(), permissions),
        "cannot change permissions of '{}'",
        dotcntr_dir.path().display()
    );
    let dotcntr_fd = try_with!(
        fcntl::open(
            dotcntr_dir.path(),
            OFlag::O_RDONLY | OFlag::O_CLOEXEC,
            stat::Mode::all(),
        ),
        "failed to open '{}' directory",
        dotcntr_dir.path().display()
    );
    let dotcntr_file = unsafe { File::from_raw_fd(dotcntr_fd) };
    let d = DotcntrDir {
        file: dotcntr_file,
        dir: dotcntr_dir,
    };
    try_with!(d.write_setcap_exe(), "failed to create setcap executable");

    try_with!(
        d.write_pid_file(process_status.local_pid),
        "failed to create pid file"
    );

    Ok(d)
}
