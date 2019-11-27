use libc;
use nix::errno::Errno;
use nix::unistd::{Gid, Uid};
use nix::{Error, NixPath, Result};
use std::ffi::CString;

pub struct Passwd {
    pub pw_name: CString,
    pub pw_passwd: CString,
    pub pw_uid: Uid,
    pub pw_gid: Gid,
    pub pw_gecos: CString,
    pub pw_dir: CString,
    pub pw_shell: CString,
}

pub fn pwnam<P: ?Sized + NixPath>(name: &P) -> Result<Option<Passwd>> {
    let res = try!(name.with_nix_path(|cstr| unsafe {
        Errno::clear();
        libc::getpwnam(cstr.as_ptr())
    }));
    if res.is_null() {
        if let Errno::UnknownErrno = Errno::last() {
            Ok(None)
        } else {
            Err(Error::Sys(Errno::last()))
        }
    } else {
        Ok(Some(unsafe {
            let res = *res;
            Passwd {
                pw_name: CString::from_raw(res.pw_name),
                pw_passwd: CString::from_raw(res.pw_passwd),
                pw_uid: Uid::from_raw(res.pw_uid),
                pw_gid: Gid::from_raw(res.pw_gid),
                pw_gecos: CString::from_raw(res.pw_gecos),
                pw_dir: CString::from_raw(res.pw_dir),
                pw_shell: CString::from_raw(res.pw_shell),
            }
        }))
    }
}
