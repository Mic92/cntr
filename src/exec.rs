use libc;
use nix::{self, unistd};
use std::env;
use std::io;
use std::os::unix::process::CommandExt;
use std::process::Command;
use types::{Error, Result};


pub fn exec(exe: &String, args: &[String]) -> Result<()> {
    let err = Command::new(exe)
        .args(args)
        .before_exec(|| {
            match unistd::chroot("/var/lib/cntr") {
                Err(nix::Error::Sys(errno)) => {
                    warn!(
                        "failed to chroot to /var/lib/cntr: {}",
                        nix::Error::Sys(errno)
                    );
                    return Err(io::Error::from(errno));
                }
                Err(e) => {
                    warn!("failed to chroot to /var/lib/cntr: {}", e);
                    return Err(io::Error::from_raw_os_error(libc::EINVAL));
                }
                _ => {}
            }

            if let Err(e) = env::set_current_dir("/") {
                warn!("failed to change directory to /");
                return Err(e);
            }

            Ok(())
        })
        .exec();
    tryfmt!(Err(err), "failed to execute `{}`", exe);
    Ok(())
}
