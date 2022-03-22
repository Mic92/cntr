extern crate cntr;
extern crate fuse;
extern crate libc;
extern crate nix;
extern crate thread_scoped;

#[cfg(feature = "profiling")]
extern crate cpuprofiler;

extern crate parking_lot;

use cntr::fs::{CntrFs, CntrMountOptions};

#[cfg(feature = "profiling")]
use cpuprofiler::PROFILER;
use nix::{mount, unistd};
use std::env;
use std::path::Path;
use std::process;

struct MountGuard {
    mount_point: String,
}

impl Drop for MountGuard {
    fn drop(&mut self) {
        let _ = mount::umount(self.mount_point.as_str());
    }
}

fn main() {
    if cfg!(feature = "verbose_fuse_test_log") {
        cntr::enable_debug_log().unwrap();
    }

    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        println!("USAGE: {} from_path to_path", args[0]);
        process::exit(1);
    }

    if let unistd::ForkResult::Parent { .. } = unistd::fork().unwrap() {
        return;
    }

    let splice_read = env::var("SPLICE_READ").is_ok();
    if splice_read {
        println!("SPLICE_READ enabled")
    }
    let splice_write = env::var("SPLICE_WRITE").is_ok();
    if splice_write {
        println!("SPLICE_WRITE enabled")
    }
    let fopen_keepcache = env::var("FOPEN_KEEPCACHE").is_ok();
    if fopen_keepcache {
        println!("FOPEN_KEEPCACHE enabled")
    }

    #[cfg(feature = "profiling")]
    PROFILER.lock().unwrap().start("./cntrfs.profile").unwrap();

    let cntr = CntrFs::new(&CntrMountOptions {
        prefix: &args[1],
        uid_map: cntr::DEFAULT_ID_MAP,
        gid_map: cntr::DEFAULT_ID_MAP,
        effective_uid: None,
        effective_gid: None,
        splice_read,
        splice_write,
        fopen_keepcache
    })
    .unwrap();

    cntr.mount(Path::new(&args[2]), &None).unwrap();
    let guard = MountGuard {
        mount_point: args[2].clone(),
    };
    cntr.spawn_sessions().unwrap();
    drop(guard);

    #[cfg(feature = "profiling")]
    PROFILER.lock().unwrap().stop().unwrap();
}
