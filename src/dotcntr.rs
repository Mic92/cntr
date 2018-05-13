use capabilities;
use libc::pid_t;
use nix::fcntl::{self, OFlag};
use nix::sys::stat;
use nix::unistd::Pid;
use procfs::ProcStatus;
use std::fs::{self, File};
use std::io::prelude::*;
use std::os::unix::prelude::*;
use tempdir::TempDir;
use types::{Error, Result};

/// Hidden directory with CAP_CHROOT enabled cntr-exec binary
pub struct DotcntrDir {
    pub file: File,
    pub dir: TempDir,
}

impl DotcntrDir {
    pub fn write_pid_file(&self, target_pid: Pid) -> Result<()> {
        let path = self.dir.path().join("pid");
        let mut file = tryfmt!(File::create(&path), "failed to create {}", path.display());

        let raw_pid: pid_t = target_pid.into();
        tryfmt!(
            file.write_all(format!("{}", raw_pid).as_bytes()),
            "failed to write {}",
            path.display()
        );
        Ok(())
    }

    pub fn write_setcap_exe(&self) -> Result<()> {
        let path = self.dir.path().join("cntr-exec");
        tryfmt!(
            fs::copy("/proc/self/exe", &path),
            "failed to copy /proc/self/exe to {}",
            path.display()
        );

        tryfmt!(
            capabilities::set_chroot_capability(&path),
            "Failed set file capability CAP_SYS_CHROOT on {}",
            path.display()
        );
        Ok(())
    }
}

pub fn create(process_status: &ProcStatus) -> Result<DotcntrDir> {
    let dotcntr_dir = tryfmt!(
        TempDir::new("dotcntr"),
        "failed to create temporary directory"
    );
    let dotcntr_fd = tryfmt!(
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
    tryfmt!(d.write_setcap_exe(), "failed to create setcap executable");

    tryfmt!(
        d.write_pid_file(process_status.local_pid),
        "failed to create pid file"
    );

    Ok(d)
}
