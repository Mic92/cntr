use libc::{self, c_int};
use nix;
use nix::errno::Errno;
use nix::sys::prctl;
use nix::unistd::Pid;
use std::fs::File;
use std::io::Read;
use std::mem;
use types::{Error, Result};

pub const _LINUX_CAPABILITY_VERSION_3: u32 = 0x2008_0522;
pub const CAP_SYS_CHROOT: u64 = 18;
pub const CAP_SYS_PTRACE: u64 = 19;

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
    user_data: [cap_user_data_t; 2],
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

fn ambient_capabilities_supported() -> bool {
    /* If PR_CAP_AMBIENT returns something valid, or an unexpected error code we assume that ambient caps are available. */
    let res = prctl::prctl(
        prctl::PrctlOption::PR_CAP_AMBIENT,
        libc::PR_CAP_AMBIENT_IS_SET as u64,
        5, // CAP_KILL
        0,
        0,
    );
    match res {
        Err(nix::Error::Sys(Errno::EINVAL)) |
        Err(nix::Error::Sys(Errno::EOPNOTSUPP)) |
        Err(nix::Error::Sys(Errno::ENOSYS)) => false,
        _ => true,
    }
}

pub fn inherit_capabilities() -> Result<()> {
    unsafe {
        let header = cap_user_header_t {
            version: _LINUX_CAPABILITY_VERSION_3,
            pid: 0,
        };

        let mut data: [cap_user_data_t; 2] = mem::uninitialized();
        let res = libc::syscall(libc::SYS_capget, &header, &mut data);
        tryfmt!(Errno::result(res), "");
        data[0].inheritable = 1 << CAP_SYS_CHROOT as u32;

        let res = libc::syscall(libc::SYS_capset, &header, &mut data);
        tryfmt!(Errno::result(res), "");

        if ambient_capabilities_supported() {
            tryfmt!(
                prctl::prctl(
                    prctl::PrctlOption::PR_CAP_AMBIENT,
                    libc::PR_CAP_AMBIENT_RAISE as u64,
                    CAP_SYS_CHROOT,
                    0,
                    0,
                ),
                "failed to keep SYS_CHROOT capability"
            );
        }
    }
    Ok(())
}

pub fn get(pid: Pid) -> Result<Capabilities> {
    let header = cap_user_header_t {
        version: _LINUX_CAPABILITY_VERSION_3,
        pid: pid.into(),
    };

    let last_capability = tryfmt!(last_capability(), "failed to get capability limit");
    let capabilities = unsafe {
        let mut data: [cap_user_data_t; 2] = mem::uninitialized();
        let res = libc::syscall(libc::SYS_capget, &header, &mut data);
        tryfmt!(Errno::result(res).map(|_| data), "")
    };

    Ok(Capabilities {
        user_data: capabilities,
        last_capability,
    })
}

impl Capabilities {
    pub fn set(&self) -> Result<()> {
        // we need chroot at the moment for `exec` command
        let mut inheritable = u64::from(self.user_data[0].inheritable);
        inheritable |= u64::from(self.user_data[1].inheritable);
        inheritable |= 1 << CAP_SYS_CHROOT | 1 << CAP_SYS_PTRACE;

        for cap in 0..self.last_capability {
            if (inheritable & (1 << cap)) == 0 {
                // TODO: do not ignore result
                let _ = prctl::prctl(prctl::PrctlOption::PR_CAPBSET_DROP, cap, 0, 0, 0);
            }
        }
        Ok(())
    }
}
