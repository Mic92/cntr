mod common;

use common::{TempDir, run_in_userns, start_fake_container};
use std::{env, path::Path};

/// Check test prerequisites: CNTR_TEST_SHELL environment variable and mount API support
///
/// Returns Some(shell_path) if all prerequisites are met, None otherwise.
fn check_test_prerequisites() -> Option<String> {
    // Check for static shell environment variable
    let static_shell = match env::var("CNTR_TEST_SHELL").ok() {
        Some(path) => path,
        None => {
            eprintln!("Skipping test: CNTR_TEST_SHELL environment variable not set");
            return None;
        }
    };

    if !Path::new(&static_shell).exists() {
        eprintln!(
            "Skipping test: CNTR_TEST_SHELL path does not exist: {}",
            static_shell
        );
        return None;
    }

    // Check for mount API support
    if !cntr::syscalls::capability::has_mount_api() {
        eprintln!("Skipping test: mount API not available on this kernel");
        return None;
    }

    Some(static_shell)
}

/// Integration test for attach flow
///
/// This test creates a fake container process with a chroot and tests that:
/// 1. cntr attach mounts the container filesystem at the configured base directory
/// 2. The container's files are accessible via the base directory
/// 3. Namespace isolation works correctly
#[test]
fn test_attach_integration() {
    let static_shell = match check_test_prerequisites() {
        Some(shell) => shell,
        None => return,
    };

    run_in_userns(|| {
        let container = start_fake_container();

        // Use a temporary directory for the test
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let base_dir = temp_dir.path().to_str().expect("Invalid temp path");

        // Run cntr attach with a verification command
        let cntr_bin = env!("CARGO_BIN_EXE_cntr");
        let pid_str = container.pid.to_string();

        let status = std::process::Command::new(cntr_bin)
            .env("CNTR_BASE_DIR", base_dir)
            .args(["attach", "-t", "process_id", &pid_str, "--"])
            .args([
                &static_shell,
                "-c",
                &format!(
                    "set -x && \
                     test -d {} && \
                     test -f {}/tmp/container-marker && \
                     test -x {}/bin/sh && \
                     grep -q fake-container {}/tmp/container-marker && \
                     echo 'All checks passed'",
                    base_dir, base_dir, base_dir, base_dir
                ),
            ])
            .status()
            .expect("Failed to execute cntr attach");

        // Check result - panic will be caught by run_in_userns
        assert!(
            status.success(),
            "Attach test failed with status: {:?}",
            status
        );
    });
}

/// Integration test for exec direct mode
///
/// This test creates a fake container and tests that cntr exec can directly
/// access it with container parameters (no daemon involved).
#[test]
fn test_exec_direct() {
    let _static_shell = match check_test_prerequisites() {
        Some(shell) => shell,
        None => return,
    };

    run_in_userns(|| {
        let container = start_fake_container();

        // Run cntr exec in direct mode with container PID
        // Note: We use /bin/sh because after chrooting to the container,
        // that's where the shell is located (copied by fake_container_process)
        // We use only shell built-ins (test, echo) since external commands may not be available
        let cntr_bin = env!("CARGO_BIN_EXE_cntr");
        let pid_str = container.pid.to_string();

        let status = std::process::Command::new(cntr_bin)
            .args(["exec", "-t", "process_id", &pid_str, "--"])
            .args([
                "/bin/sh",
                "-c",
                "test -f /tmp/container-marker && echo 'Exec direct test passed'",
            ])
            .status()
            .expect("Failed to execute cntr exec");

        // Check result - panic will be caught by run_in_userns
        assert!(
            status.success(),
            "Exec direct test failed with status: {:?}",
            status
        );
    });
}
