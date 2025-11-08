mod common;

use common::{run_in_userns, start_fake_container};
use nix::sys::signal::{Signal, kill};
use nix::sys::wait::waitpid;
use std::{env, path::Path};

/// Integration test for attach flow
///
/// This test creates a fake container process with a chroot and tests that:
/// 1. cntr attach mounts the container filesystem at /var/lib/cntr
/// 2. The container's files are accessible via /var/lib/cntr/*
/// 3. Namespace isolation works correctly
#[test]
fn test_attach_integration() {
    // Check for static shell environment variable
    let static_shell = match env::var("CNTR_TEST_SHELL") {
        Ok(path) => path,
        Err(_) => {
            eprintln!("Skipping test: CNTR_TEST_SHELL environment variable not set");
            return;
        }
    };

    if !Path::new(&static_shell).exists() {
        eprintln!(
            "Skipping test: CNTR_TEST_SHELL path does not exist: {}",
            static_shell
        );
        return;
    }

    // Check for mount API support
    if !cntr::syscalls::capability::has_mount_api() {
        eprintln!("Skipping test: mount API not available on this kernel");
        return;
    }

    run_in_userns(|| {
        let container = start_fake_container();

        // Run cntr attach with a verification command
        let cntr_bin = env!("CARGO_BIN_EXE_cntr");
        let pid_str = container.pid.to_string();

        let status = std::process::Command::new(cntr_bin)
            .args(&["attach", "-t", "process-id", &pid_str, "--"])
            .args(&[
                &static_shell,
                "-c",
                "set -x && \
                 test -d /var/lib/cntr && \
                 test -f /var/lib/cntr/tmp/container-marker && \
                 test -x /var/lib/cntr/bin/sh && \
                 grep -q fake-container /var/lib/cntr/tmp/container-marker && \
                 echo 'All checks passed'",
            ])
            .status()
            .expect("Failed to execute cntr attach");

        // Cleanup: kill container (FakeContainer will clean up temp dir on drop)
        let _ = kill(container.pid, Signal::SIGTERM);
        let _ = waitpid(container.pid, None);

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
    // Check for static shell environment variable
    let static_shell = match env::var("CNTR_TEST_SHELL") {
        Ok(path) => path,
        Err(_) => {
            eprintln!("Skipping test: CNTR_TEST_SHELL environment variable not set");
            return;
        }
    };

    if !Path::new(&static_shell).exists() {
        eprintln!(
            "Skipping test: CNTR_TEST_SHELL path does not exist: {}",
            static_shell
        );
        return;
    }

    // Check for mount API support
    if !cntr::syscalls::capability::has_mount_api() {
        eprintln!("Skipping test: mount API not available on this kernel");
        return;
    }

    run_in_userns(|| {
        let container = start_fake_container();

        // Run cntr exec in direct mode with container PID
        // Note: We use /bin/sh because after chrooting to the container,
        // that's where the shell is located (copied by fake_container_process)
        // We use only shell built-ins (test, echo) since external commands may not be available
        let cntr_bin = env!("CARGO_BIN_EXE_cntr");
        let pid_str = container.pid.to_string();

        let status = std::process::Command::new(cntr_bin)
            .args(&["exec", "-t", "process-id", &pid_str, "--"])
            .args(&[
                "/bin/sh",
                "-c",
                "test -f /tmp/container-marker && echo 'Exec direct test passed'",
            ])
            .status()
            .expect("Failed to execute cntr exec");

        // Cleanup: kill container (FakeContainer will clean up temp dir on drop)
        let _ = kill(container.pid, Signal::SIGTERM);
        let _ = waitpid(container.pid, None);

        // Check result - panic will be caught by run_in_userns
        assert!(
            status.success(),
            "Exec direct test failed with status: {:?}",
            status
        );
    });
}

/// Integration test for exec daemon mode
///
/// This test creates a fake container, attaches to it, and calls cntr exec
/// from within the attach command to test daemon communication.
#[test]
fn test_exec_daemon() {
    // Check for static shell environment variable
    let static_shell = match env::var("CNTR_TEST_SHELL") {
        Ok(path) => path,
        Err(_) => {
            eprintln!("Skipping test: CNTR_TEST_SHELL environment variable not set");
            return;
        }
    };

    if !Path::new(&static_shell).exists() {
        eprintln!(
            "Skipping test: CNTR_TEST_SHELL path does not exist: {}",
            static_shell
        );
        return;
    }

    // Check for mount API support
    if !cntr::syscalls::capability::has_mount_api() {
        eprintln!("Skipping test: mount API not available on this kernel");
        return;
    }

    run_in_userns(|| {
        let container = start_fake_container();
        let cntr_bin = env!("CARGO_BIN_EXE_cntr");
        let pid_str = container.pid.to_string();

        // Run cntr attach, which within it calls cntr exec to test daemon mode
        let status = std::process::Command::new(cntr_bin)
            .args(&["attach", "-t", "process-id", &pid_str, "--"])
            .args(&[
                &static_shell,
                "-c",
                &format!(
                    "{} exec -- /bin/sh -c 'test -f /tmp/container-marker && echo Exec daemon test passed'",
                    cntr_bin
                ),
            ])
            .status()
            .expect("Failed to execute cntr attach");

        // Cleanup: kill container (FakeContainer will clean up temp dir on drop)
        let _ = kill(container.pid, Signal::SIGTERM);
        let _ = waitpid(container.pid, None);

        // Check result - panic will be caught by run_in_userns
        assert!(
            status.success(),
            "Exec daemon test failed with status: {:?}",
            status
        );
    });
}
