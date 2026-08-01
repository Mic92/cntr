use libc::{self, c_int, c_ulong};
use std::io;

/// Apply an operation on a process
/// [prctl(2)](http://man7.org/linux/man-pages/man2/prctl.2.html)
///
/// prctl is called with a first argument describing what to do,
/// further arguments with a significance depending on the first one.
pub(crate) fn prctl(
    option: c_int,
    arg2: c_ulong,
    arg3: c_ulong,
    arg4: c_ulong,
    arg5: c_ulong,
) -> io::Result<()> {
    let res = unsafe { libc::prctl(option, arg2, arg3, arg4, arg5) };

    if res == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
