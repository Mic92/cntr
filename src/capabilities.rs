use libc::{self, c_int};
use nix::errno::Errno;
use nix::sys::prctl;
use nix::unistd::Pid;
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
struct cap_user_data_t {
    effective: u32,
    permitted: u32,
    inheritable: u32,
}

pub struct Capabilities {
    user_data: cap_user_data_t,
    last_capability: u64,
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

pub fn get(pid: Option<Pid>) -> Result<Capabilities> {
    let header = cap_user_header_t {
        version: _LINUX_CAPABILITY_VERSION_3,
        pid: pid.map_or(0, Into::into),
    };

    let last_capability = tryfmt!(last_capability(), "failed to get capability limit");
    let capabilities = unsafe {
        let mut data: cap_user_data_t = mem::uninitialized();
        let res = libc::syscall(libc::SYS_capget, &header, &mut data);
        tryfmt!(Errno::result(res).map(|_| data), "")
    };

    Ok(Capabilities {
        user_data: capabilities,
        last_capability: last_capability,
    })
}

impl Capabilities {
    pub fn set(&self, pid: Option<Pid>) -> Result<()> {
        let header = cap_user_header_t {
            version: _LINUX_CAPABILITY_VERSION_3,
            pid: pid.map_or(0, Into::into),
        };
        let res = unsafe { libc::syscall(libc::SYS_capset, &header, &self.user_data) };
        tryfmt!(Errno::result(res).map(drop), "");

        for cap in 0..self.last_capability {
            if (u64::from(self.user_data.effective)) & (1 << cap) == 0 {
                // TODO: do not ignore result
                let _ = prctl::prctl(prctl::PrctlOption::PR_CAPBSET_DROP, cap, 0, 0, 0);
            }
        }
        Ok(())
    }
}
