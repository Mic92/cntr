use nix;
use nix::errno::Errno;
use nix::fcntl::AtFlags;
use nix::unistd;
use std::os::unix::prelude::RawFd;

// According to the POSIX, -1 is used to indicate that
// owner and group, respectively, are not to be changed. Since uid_t and
// gid_t are unsigned types, we use wrapping_sub to get '-1'.
fn optional_user(val: Option<unistd::Uid>) -> u32 {
    val.map(Into::into)
        .unwrap_or((0 as libc::uid_t).wrapping_sub(1))
}

fn optional_group(val: Option<unistd::Gid>) -> u32 {
    val.map(Into::into)
        .unwrap_or((0 as libc::gid_t).wrapping_sub(1))
}

/// Change ownership of a file
/// (see [fchownat(2)](http://man7.org/linux/man-pages/man2/fchownat.2.html)).
pub fn fchownat<P: ?Sized + nix::NixPath>(
    dirfd: RawFd,
    pathname: &P,
    owner: Option<unistd::Uid>,
    group: Option<unistd::Gid>,
    flags: AtFlags,
) -> nix::Result<()> {
    let res = pathname.with_nix_path(|cstr| unsafe {
        libc::fchownat(
            dirfd,
            cstr.as_ptr(),
            optional_user(owner),
            optional_group(group),
            flags.bits(),
        )
    })?;

    Errno::result(res).map(drop)
}
