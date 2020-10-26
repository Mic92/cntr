use nix;
use nix::errno::Errno;
use nix::sys::stat;
use std::os::unix::prelude::RawFd;

/// Create a special or ordinary file
/// ([posix specification](http://pubs.opengroup.org/onlinepubs/9699919799/functions/mknod.html)).
#[cfg(not(any(target_os = "ios", target_os = "macos")))]
pub fn mknodat<P: ?Sized + nix::NixPath>(
    dirfd: &RawFd,
    path: &P,
    kind: stat::SFlag,
    perm: stat::Mode,
    dev: libc::dev_t,
) -> nix::Result<()> {
    let res = path.with_nix_path(|cstr| unsafe {
        libc::mknodat(
            *dirfd,
            cstr.as_ptr(),
            kind.bits() | perm.bits() as libc::mode_t,
            dev,
        )
    })?;

    Errno::result(res).map(drop)
}
