use cntr::fs::{CntrFs, CntrMountOptions};

#[cfg(feature = "profiling")]
use cpuprofiler::PROFILER;
use nix::sys::signal;
use nix::{mount, unistd};
use simple_error::{try_with, SimpleError};
use std::env;
use std::sync::Mutex;
use log::{info, error};
use std::path::Path;
use std::process;
use std::sync::mpsc::{SyncSender, sync_channel};
use lazy_static::lazy_static;

struct MountGuard {
    mount_point: String,
}

impl Drop for MountGuard {
    fn drop(&mut self) {
        let _ = mount::umount(self.mount_point.as_str());
    }
}

pub type Result<T> = std::result::Result<T, SimpleError>;

lazy_static! {
    static ref SIGNAL_SENDER: Mutex<Option<SyncSender<()>>> = Mutex::new(None);
}

extern "C" fn signal_handler(_: ::libc::c_int) {
    let sender = match SIGNAL_SENDER.lock().expect("cannot lock sender").take() {
        Some(s) => {
            info!("shutdown cntrfs");
            s
        }
        None => {
            info!("received sigterm. stopping already in progress");
            return;
        }
    };
    if let Err(e) = sender.send(()) {
        error!("cannot notify main process: {}", e);
    }
}

pub fn setup_signal_handler(sender: SyncSender<()>) -> Result<()> {
    try_with!(SIGNAL_SENDER.lock(), "cannot get lock").replace(sender);

    let sig_action = signal::SigAction::new(
        signal::SigHandler::Handler(signal_handler),
        signal::SaFlags::empty(),
        signal::SigSet::empty(),
    );

    unsafe {
        try_with!(
            signal::sigaction(signal::SIGINT, &sig_action),
            "unable to register SIGINT handler"
        );
        try_with!(
            signal::sigaction(signal::SIGTERM, &sig_action),
            "unable to register SIGTERM handler"
        );
    }
    Ok(())
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
    let res = unsafe { unistd::fork().unwrap() };

    if let unistd::ForkResult::Parent { .. } = res {
        return;
    }

    #[cfg(feature = "profiling")]
    PROFILER.lock().unwrap().start("./cntrfs.profile").unwrap();

    let cntr = CntrFs::new(
        &CntrMountOptions {
            prefix: &args[1],
            uid_map: cntr::DEFAULT_ID_MAP,
            gid_map: cntr::DEFAULT_ID_MAP,
            effective_uid: None,
            effective_gid: None,
        },
        None,
    )
    .unwrap();

    cntr.mount(Path::new(&args[2]), &None).unwrap();
    let guard = MountGuard {
        mount_point: args[2].clone(),
    };
    cntr.spawn_sessions().unwrap();
    let (sender, receiver) = sync_channel(1);
    setup_signal_handler(sender).unwrap();
    // wait for exit signal
    let _ = receiver.recv();

    drop(guard);

    #[cfg(feature = "profiling")]
    PROFILER.lock().unwrap().stop().unwrap();
}
