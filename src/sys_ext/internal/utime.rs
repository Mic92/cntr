use nix::errno::Errno;
use nix::fcntl::AtFlags;
use nix::sys::time::TimeSpec;
use std::os::unix::io::RawFd;

/// A file timestamp.
#[derive(Clone, Copy, Debug)]
pub enum UtimeSpec {
    /// File timestamp is set to the current time.
    Now,
    /// The corresponding file timestamp is left unchanged.
    Omit,
    /// File timestamp is set to value
    Time(TimeSpec),
}

impl<'a> From<&'a UtimeSpec> for libc::timespec {
    fn from(time: &'a UtimeSpec) -> libc::timespec {
        // Debian: In Debian packaged rust-libc 0.2.153-2, a private pad field
        // was added to libc::timespec for specific architectures, failing the
        // original struct literal syntax.
        let mut spec: libc::timespec = unsafe { std::mem::zeroed() };
        match time {
            UtimeSpec::Now => {
                spec.tv_nsec = libc::UTIME_NOW;
            }
            UtimeSpec::Omit => {
                spec.tv_nsec = libc::UTIME_OMIT;
            }
            UtimeSpec::Time(timespec) => return *timespec.as_ref(),
        }
        spec
    }
}

/// Change file timestamps with nanosecond precision
/// (see [utimensat(2)](http://man7.org/linux/man-pages/man2/utimensat.2.html)).
pub fn utimensat<P: ?Sized + nix::NixPath>(
    dirfd: RawFd,
    pathname: &P,
    atime: &UtimeSpec,
    mtime: &UtimeSpec,
    flags: AtFlags,
) -> nix::Result<()> {
    let time = [atime.into(), mtime.into()];
    let res = pathname.with_nix_path(|cstr| unsafe {
        libc::utimensat(dirfd, cstr.as_ptr(), time.as_ptr(), flags.bits())
    })?;

    Errno::result(res).map(drop)
}

/// Change file timestamps with nanosecond precision
/// (see [futimens(2)](http://man7.org/linux/man-pages/man2/futimens.2.html)).
pub fn futimens(fd: RawFd, atime: &UtimeSpec, mtime: &UtimeSpec) -> nix::Result<()> {
    let time = [atime.into(), mtime.into()];
    let res = unsafe { libc::futimens(fd, time.as_ptr()) };

    Errno::result(res).map(drop)
}
