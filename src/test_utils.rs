//! Test utilities shared between unit and integration tests

use nix::sys::wait::{WaitStatus, waitpid};
use nix::unistd::{ForkResult, fork};

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
            // Wait for test to complete
            match waitpid(child, None) {
                Ok(WaitStatus::Exited(_, 0)) => {
                    // Test passed
                }
                Ok(WaitStatus::Exited(_, code)) => {
                    panic!("Test failed with exit code {}", code);
                }
                Ok(status) => {
                    panic!("Test process terminated abnormally: {:?}", status);
                }
                Err(e) => {
                    panic!("waitpid failed: {}", e);
                }
            }
        }
        Err(e) => {
            panic!("Failed to fork for user namespace: {}", e);
        }
    }
}
