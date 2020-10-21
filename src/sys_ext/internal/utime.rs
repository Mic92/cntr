use libc;
use nix;
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
        match time {
            &UtimeSpec::Now => libc::timespec {
                tv_sec: 0,
                tv_nsec: libc::UTIME_NOW,
            },
            &UtimeSpec::Omit => libc::timespec {
                tv_sec: 0,
                tv_nsec: libc::UTIME_OMIT,
            },
            &UtimeSpec::Time(spec) => *spec.as_ref(),
        }
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
        libc::utimensat(
            dirfd,
            cstr.as_ptr(),
            time.as_ptr() as *const libc::timespec,
            flags.bits(),
        )
    })?;

    Errno::result(res).map(drop)
}

/// Change file timestamps with nanosecond precision
/// (see [futimens(2)](http://man7.org/linux/man-pages/man2/futimens.2.html)).
pub fn futimens(fd: RawFd, atime: &UtimeSpec, mtime: &UtimeSpec) -> nix::Result<()> {
    let time = [atime.into(), mtime.into()];
    let res = unsafe { libc::futimens(fd, time.as_ptr() as *const libc::timespec) };

    Errno::result(res).map(drop)
}
