//! Test utilities shared between unit and integration tests

use nix::sys::signal::{Signal, kill};
use nix::unistd::{ForkResult, Pid, fork};
use std::time::{Duration, Instant};
use std::thread;

/// Wait for a child process with timeout protection
///
/// This function polls the child process with WNOHANG and tracks elapsed time.
/// If the timeout is exceeded, it sends SIGTERM, waits for a grace period,
/// then sends SIGKILL if needed before reaping the child.
///
/// # Panics
///
/// Panics if:
/// - The child times out (after attempting SIGTERM then SIGKILL)
/// - waitpid encounters an error
/// - Unable to send kill signals
fn wait_child_with_timeout(child: Pid, timeout: Duration) -> WaitStatus {
    const GRACE_PERIOD: Duration = Duration::from_secs(2);
    const POLL_INTERVAL: Duration = Duration::from_millis(100);

    let start = Instant::now();
    let mut sent_sigterm = false;
    let mut sigterm_time: Option<Instant> = None;

    loop {
        match waitpid(child, Some(WaitPidFlag::WNOHANG)) {
            Ok(WaitStatus::StillAlive) => {
                let elapsed = start.elapsed();

                if elapsed >= timeout {
                    if !sent_sigterm {
                        // First, try SIGTERM for graceful shutdown
                        if let Err(e) = kill(child, Signal::SIGTERM) {
                            panic!(
                                "Test timeout: failed to send SIGTERM to child {}: {}",
                                child, e
                            );
                        }
                        sent_sigterm = true;
                        sigterm_time = Some(Instant::now());
                    } else if sigterm_time.is_some_and(|t| t.elapsed() >= GRACE_PERIOD) {
                        // Grace period expired, forcefully kill with SIGKILL
                        if let Err(e) = kill(child, Signal::SIGKILL) {
                            // ESRCH is OK - child may have exited after SIGTERM
                            if e != nix::errno::Errno::ESRCH {
                                panic!(
                                    "Test timeout: failed to send SIGKILL to child {}: {}",
                                    child, e
                                );
                            }
                        }

                        // Reap the child process
                        match waitpid(child, None) {
                            Ok(_) => {
                                panic!(
                                    "Test timed out after {:.2} seconds (child PID: {})",
                                    elapsed.as_secs_f64(),
                                    child
                                );
                            }
                            Err(e) => {
                                panic!(
                                    "Test timed out after {:.2} seconds and waitpid failed after SIGKILL: {} (child PID: {})",
                                    elapsed.as_secs_f64(),
                                    e,
                                    child
                                );
                            }
                        }
                    }
                }

                thread::sleep(POLL_INTERVAL);
            }
            Ok(status) => {
                // Child has exited or terminated
                return status;
            }
            Err(e) => {
                panic!("waitpid failed: {}", e);
            }
        }
    }
}

/// Run a test function in a user namespace
///
/// This creates a new user namespace and runs the provided function.
/// The function runs in a forked child process, which waits for completion.
pub fn run_in_userns<F>(test_fn: F)
where
    F: FnOnce(),
{
    match unsafe { fork() } {
        Ok(ForkResult::Child) => {
            // Get current UID/GID before unshare
            let uid = nix::unistd::getuid();
            let gid = nix::unistd::getgid();

            // Create user and mount namespaces
            use nix::sched::{CloneFlags, unshare};
            if let Err(e) = unshare(CloneFlags::CLONE_NEWUSER | CloneFlags::CLONE_NEWNS) {
                eprintln!("Failed to create user/mount namespace: {}", e);
                unsafe { libc::_exit(1) };
            }

            // Set up UID/GID mappings
            // Map our current UID to 0 (root) in the new namespace
            if let Err(e) = std::fs::write("/proc/self/setgroups", b"deny") {
                eprintln!("Failed to write setgroups: {}", e);
                unsafe { libc::_exit(1) };
            }

            let uid_map = format!("0 {} 1\n", uid);
            if let Err(e) = std::fs::write("/proc/self/uid_map", uid_map.as_bytes()) {
                eprintln!("Failed to write uid_map: {}", e);
                unsafe { libc::_exit(1) };
            }

            let gid_map = format!("0 {} 1\n", gid);
            if let Err(e) = std::fs::write("/proc/self/gid_map", gid_map.as_bytes()) {
                eprintln!("Failed to write gid_map: {}", e);
                unsafe { libc::_exit(1) };
            }

            // Run the test - panics will cause child to exit with non-zero
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                test_fn();
            }));

            match result {
                Ok(_) => unsafe { libc::_exit(0) },
                Err(_) => unsafe { libc::_exit(1) },
            }
        }
        Ok(ForkResult::Parent { child }) => {
            // Wait for test to complete with timeout protection
            match wait_child_with_timeout(child, Duration::from_secs(30)) {
                WaitStatus::Exited(_, 0) => {
                    // Test passed
                }
                WaitStatus::Exited(_, code) => {
                    panic!("Test failed with exit code {}", code);
                }
                status => {
                    panic!("Test process terminated abnormally: {:?}", status);
                }
            }
        }
        Err(e) => {
            panic!("Failed to fork for user namespace: {}", e);
        }
    }
}
