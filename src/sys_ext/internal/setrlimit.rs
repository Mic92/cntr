pub use libc::rlimit64 as Rlimit;

use nix::errno::Errno;

#[cfg(target_env = "gnu")]
pub fn setrlimit(resource: libc::c_uint, rlimit: &Rlimit) -> nix::Result<()> {
    let res = unsafe { libc::setrlimit64(resource, rlimit as *const Rlimit) };
    Errno::result(res).map(drop)
}

#[cfg(not(target_env = "gnu"))]
pub fn setrlimit(resource: libc::c_int, rlimit: &Rlimit) -> nix::Result<()> {
    let res = unsafe { libc::setrlimit64(resource, rlimit as *const Rlimit) };
    Errno::result(res).map(drop)
}
