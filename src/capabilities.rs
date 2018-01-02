use cntr_nix::errno::Errno;
use cntr_nix::sys::prctl;
use cntr_nix::unistd::Pid;
use libc::{self, c_int};
use std::fs::File;
use std::io::Read;
use std::mem;
use types::{Error, Result};

pub const _LINUX_CAPABILITY_VERSION_3: u32 = 0x2008_0522;

#[repr(C)]
struct cap_user_header_t {
    version: u32,
    pid: c_int,
}

#[repr(C)]
pub struct cap_user_data_t {
    pub effective: u32,
    pub permitted: u32,
    pub inheritable: u32,
}

fn last_capability() -> Result<u64> {
    let path = "/proc/sys/kernel/cap_last_cap";
    let mut f = tryfmt!(File::open(path), "failed to open {}", path);

    let mut contents = String::new();
    tryfmt!(f.read_to_string(&mut contents), "failed to read {}", path);
    contents.pop(); // remove newline
    Ok(tryfmt!(
        contents.parse::<u64>(),
        "failed to parse capability, got: '{}'",
        contents
    ))
}

pub fn get(pid: Option<Pid>) -> Result<cap_user_data_t> {
    let header = cap_user_header_t {
        version: _LINUX_CAPABILITY_VERSION_3,
        pid: pid.map_or(0, Into::into),
    };
    unsafe {
        let mut data: cap_user_data_t = mem::uninitialized();
        let res = libc::syscall(libc::SYS_capget, &header, &mut data);
        Ok(tryfmt!(Errno::result(res).map(|_| data), ""))
    }
}

pub fn set(pid: Option<Pid>, data: &cap_user_data_t) -> Result<()> {
    let header = cap_user_header_t {
        version: _LINUX_CAPABILITY_VERSION_3,
        pid: pid.map_or(0, Into::into),
    };
    let res = unsafe { libc::syscall(libc::SYS_capset, &header, data) };
    tryfmt!(Errno::result(res).map(drop), "");

    let last = tryfmt!(last_capability(), "failed to get capability limit");

    for cap in 0..last {
        if (u64::from(data.effective)) & (1 << cap) == 0 {
            // TODO: do not ignore result
            let _ = prctl::prctl(prctl::PrctlOption::PR_CAPBSET_DROP, cap, 0, 0, 0);
        }
    }
    Ok(())
}
