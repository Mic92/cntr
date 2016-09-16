use libc;
use nix::sys::signal;
use std::ffi::CStr;
use std::fmt;
use std::str::from_utf8_unchecked;

extern "C" {
    fn strsignal(sig: libc::c_int) -> *mut libc::c_char;
}

pub struct Signal {
    pub n: signal::Signal,
}

impl fmt::Display for Signal {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        // not reentrant safe in glibc
        write!(f,
               "{}",
               unsafe { from_utf8_unchecked(CStr::from_ptr(strsignal(self.n as i32)).to_bytes()) })
    }
}
