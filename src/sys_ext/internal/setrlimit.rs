pub use libc::rlimit64 as Rlimit;

use libc;
use nix;
use nix::errno::Errno;

pub fn setrlimit(resource: libc::c_uint, rlimit: &Rlimit) -> nix::Result<()> {
    let res = unsafe { libc::setrlimit64(resource, rlimit as *const Rlimit) };
    Errno::result(res).map(drop)
}
