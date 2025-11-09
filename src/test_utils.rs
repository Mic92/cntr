//! Test utilities shared between unit and integration tests

use nix::sys::signal::{Signal, kill};
use nix::sys::wait::{WaitPidFlag, WaitStatus, waitpid};
use nix::unistd::{ForkResult, Pid, fork, pipe, write};
use std::backtrace::Backtrace;
use std::fs::File;
use std::io::Read;
use std::thread;
use std::time::{Duration, Instant};

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
    // Create pipe for passing panic messages from child to parent
    let (read_fd, write_fd) = pipe().expect("Failed to create pipe for panic messages");

    match unsafe { fork() } {
        Ok(ForkResult::Child) => {
            // Close read end - child only writes
            drop(read_fd);

            // Run the test - capture and propagate panic messages (including setup failures)
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                // Get current UID/GID before unshare
                let uid = nix::unistd::getuid();
                let gid = nix::unistd::getgid();

                // Create user and mount namespaces
                use nix::sched::{CloneFlags, unshare};
                unshare(CloneFlags::CLONE_NEWUSER | CloneFlags::CLONE_NEWNS)
                    .expect("Failed to create user/mount namespace");

                // Set up UID/GID mappings
                // Map our current UID to 0 (root) in the new namespace
                std::fs::write("/proc/self/setgroups", b"deny").expect("Failed to write setgroups");

                let uid_map = format!("0 {} 1\n", uid);
                std::fs::write("/proc/self/uid_map", uid_map.as_bytes())
                    .expect("Failed to write uid_map");

                let gid_map = format!("0 {} 1\n", gid);
                std::fs::write("/proc/self/gid_map", gid_map.as_bytes())
                    .expect("Failed to write gid_map");

                // Run the actual test
                test_fn();
            }));

            match result {
                Ok(_) => {
                    drop(write_fd);
                    unsafe { libc::_exit(0) }
                }
                Err(panic_payload) => {
                    // Extract panic message and backtrace
                    let panic_msg = if let Some(s) = panic_payload.downcast_ref::<&str>() {
                        s.to_string()
                    } else if let Some(s) = panic_payload.downcast_ref::<String>() {
                        s.clone()
                    } else {
                        "Box<dyn Any>".to_string()
                    };
                    let backtrace = Backtrace::capture();

                    let full_message =
                        if backtrace.status() == std::backtrace::BacktraceStatus::Captured {
                            format!("Panic: {}\n\nBacktrace:\n{}", panic_msg, backtrace)
                        } else {
                            format!("Panic: {}", panic_msg)
                        };

                    // Write panic message to pipe
                    let msg_bytes = full_message.as_bytes();
                    let _ = write(&write_fd, msg_bytes);
                    drop(write_fd);
                    unsafe { libc::_exit(1) }
                }
            }
        }
        Ok(ForkResult::Parent { child }) => {
            // Close write end - parent only reads
            drop(write_fd);

            // Wait for test to complete with timeout protection
            let wait_result = wait_child_with_timeout(child, Duration::from_secs(30));

            // Read any panic message from the pipe
            let mut panic_data = Vec::new();
            let _ = File::from(read_fd).read_to_end(&mut panic_data);

            let panic_message = if !panic_data.is_empty() {
                String::from_utf8_lossy(&panic_data).to_string()
            } else {
                String::new()
            };

            match wait_result {
                WaitStatus::Exited(_, 0) => {
                    // Test passed
                }
                WaitStatus::Exited(_, code) => {
                    if !panic_message.is_empty() {
                        panic!("Test failed with exit code {}:\n{}", code, panic_message);
                    } else {
                        panic!("Test failed with exit code {}", code);
                    }
                }
                status => {
                    if !panic_message.is_empty() {
                        panic!(
                            "Test process terminated abnormally: {:?}\n{}",
                            status, panic_message
                        );
                    } else {
                        panic!("Test process terminated abnormally: {:?}", status);
                    }
                }
            }
        }
        Err(e) => {
            drop(read_fd);
            drop(write_fd);
            panic!("Failed to fork for user namespace: {}", e);
        }
    }
}
