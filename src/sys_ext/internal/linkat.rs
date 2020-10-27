use nix::errno::Errno;
use nix::fcntl::AtFlags;
use std::os::unix::prelude::RawFd;

/// Call the link function to create a link to a file
/// ([posix specification](http://pubs.opengroup.org/onlinepubs/9699919799/functions/linkat.html)).
pub fn linkat<P1: ?Sized + nix::NixPath, P2: ?Sized + nix::NixPath>(
    olddirfd: RawFd,
    oldpath: &P1,
    newdirfd: RawFd,
    newpath: &P2,
    flags: AtFlags,
) -> nix::Result<()> {
    let res = oldpath.with_nix_path(|old| {
        newpath.with_nix_path(|new| unsafe {
            libc::linkat(
                olddirfd,
                old.as_ptr() as *const libc::c_char,
                newdirfd,
                new.as_ptr() as *const libc::c_char,
                flags.bits(),
            )
        })
    })??;

    Errno::result(res).map(drop)
}
