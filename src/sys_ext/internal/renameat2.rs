use libc;
use nix;
use nix::errno::Errno;
use std::os::unix::prelude::RawFd;

#[cfg(any(target_os = "android", target_os = "linux"))]
pub fn renameat2<P1: ?Sized + nix::NixPath, P2: ?Sized + nix::NixPath>(
    olddirfd: RawFd,
    oldpath: &P1,
    newdirfd: RawFd,
    newpath: &P2,
    flags: libc::c_uint,
) -> nix::Result<()> {
    let res = oldpath.with_nix_path(|old| {
        newpath.with_nix_path(|new| unsafe {
            libc::syscall(
                libc::SYS_renameat2,
                olddirfd,
                old.as_ptr(),
                newdirfd,
                new.as_ptr(),
                flags,
            )
        })
    })??;

    Errno::result(res).map(drop)
}
