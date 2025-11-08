//! Common test utilities for integration tests

use nix::unistd::{ForkResult, Pid, fork, pause};
use std::{env, ffi::OsString, os::unix::ffi::OsStringExt, path::Path, path::PathBuf};

// Re-export from library
pub(crate) use cntr::test_utils::run_in_userns;

/// Simple temporary directory that cleans up on drop
pub(crate) struct TempDir {
    path: Option<PathBuf>,
}

impl TempDir {
    /// Create a new temporary directory using mkdtemp
    pub(crate) fn new() -> std::io::Result<Self> {
        let mut template = env::temp_dir();
        template.push("cntr-test.XXXXXX");
        let mut bytes = template.into_os_string().into_vec();
        // null byte
        bytes.push(0);
        let res = unsafe { libc::mkdtemp(bytes.as_mut_ptr().cast()) };
        if res.is_null() {
            Err(std::io::Error::last_os_error())
        } else {
            // remove null byte
            bytes.pop();
            let path = PathBuf::from(OsString::from_vec(bytes));
            Ok(TempDir { path: Some(path) })
        }
    }

    /// Get the path to the temporary directory
    pub(crate) fn path(&self) -> &Path {
        self.path.as_ref().unwrap()
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        if let Some(ref p) = self.path {
            let _ = std::fs::remove_dir_all(p);
        }
    }
}

/// Container info with cleanup handle
pub(crate) struct FakeContainer {
    pub(crate) pid: Pid,
    _temp_dir: TempDir,
}

// TempDir auto-cleans on drop, no manual Drop needed

/// Get the container PID and wait for it to be ready
pub(crate) fn start_fake_container() -> FakeContainer {
    use nix::unistd::pipe;
    use std::io::Read;
    use std::os::fd::{AsRawFd, FromRawFd, IntoRawFd};

    // Create temp directory in parent (before fork)
    let temp_dir_handle = TempDir::new().expect("Failed to create temp directory");
    let temp_dir_path = temp_dir_handle.path().to_path_buf();

    // Create pipe for synchronization
    let (read_fd, write_fd) = match pipe() {
        Ok(fds) => fds,
        Err(e) => panic!("Failed to create pipe: {}", e),
    };

    match unsafe { fork() } {
        Ok(ForkResult::Child) => {
            // Close read end in child
            drop(read_fd);

            // Do container setup
            fake_container_process_with_sync(write_fd.as_raw_fd(), &temp_dir_path);
        }
        Ok(ForkResult::Parent { child }) => {
            // Close write end in parent
            drop(write_fd);

            // Wait for child to signal readiness by closing its write end
            let mut read_file = unsafe { std::fs::File::from_raw_fd(read_fd.as_raw_fd()) };
            let mut buf = [0u8; 1];

            // This blocks until child closes write_fd (returns 0 bytes = EOF)
            match read_file.read(&mut buf) {
                Ok(0) => {
                    // Child closed the pipe - it's ready
                }
                Ok(_) => {
                    panic!("Unexpected data from container setup pipe");
                }
                Err(e) => {
                    panic!("Failed to read from sync pipe: {}", e);
                }
            }

            // Don't close read_fd twice
            let _ = read_file.into_raw_fd();

            FakeContainer {
                pid: child,
                _temp_dir: temp_dir_handle,
            }
        }
        Err(e) => {
            panic!("Failed to fork container process: {}", e);
        }
    }
}

/// Helper that does container setup and signals completion
fn fake_container_process_with_sync(sync_fd: std::os::fd::RawFd, temp_dir: &std::path::Path) -> ! {
    // Get static shell from environment
    let static_shell = match env::var("CNTR_TEST_SHELL") {
        Ok(path) => path,
        Err(_) => {
            eprintln!("CNTR_TEST_SHELL not set in fake_container_process");
            unsafe { libc::_exit(1) };
        }
    };

    if !Path::new(&static_shell).exists() {
        eprintln!("CNTR_TEST_SHELL path does not exist: {}", static_shell);
        unsafe { libc::_exit(1) };
    }

    // Create mount namespace only
    // The parent test already created a user namespace, so we don't need another
    use nix::sched::{CloneFlags, unshare};
    if let Err(e) = unshare(CloneFlags::CLONE_NEWNS) {
        eprintln!("Failed to create mount namespace: {}", e);
        unsafe { libc::_exit(1) };
    }

    // Make all mounts private (MS_REC | MS_PRIVATE)
    use nix::mount::{MsFlags, mount};
    if let Err(e) = mount(
        None::<&str>,
        "/",
        None::<&str>,
        MsFlags::MS_REC | MsFlags::MS_PRIVATE,
        None::<&str>,
    ) {
        eprintln!("Failed to make mounts private: {}", e);
        unsafe { libc::_exit(1) };
    }

    // Create /bin and /tmp in chroot
    let bin_dir = temp_dir.join("bin");
    let tmp_dir = temp_dir.join("tmp");

    if let Err(e) = std::fs::create_dir_all(&bin_dir) {
        eprintln!("Failed to create bin dir: {}", e);
        unsafe { libc::_exit(1) };
    }

    if let Err(e) = std::fs::create_dir_all(&tmp_dir) {
        eprintln!("Failed to create tmp dir: {}", e);
        unsafe { libc::_exit(1) };
    }

    // Copy static shell to /bin/sh
    let shell_dest = bin_dir.join("sh");
    if let Err(e) = std::fs::copy(&static_shell, &shell_dest) {
        eprintln!("Failed to copy shell: {}", e);
        unsafe { libc::_exit(1) };
    }

    // Make shell executable
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Err(e) =
            std::fs::set_permissions(&shell_dest, std::fs::Permissions::from_mode(0o755))
        {
            eprintln!("Failed to make shell executable: {}", e);
            unsafe { libc::_exit(1) };
        }
    }

    // Create marker file
    let marker = tmp_dir.join("container-marker");
    if let Err(e) = std::fs::write(&marker, b"fake-container\n") {
        eprintln!("Failed to create marker: {}", e);
        unsafe { libc::_exit(1) };
    }

    // Chroot to temp directory
    use nix::unistd::chroot;
    if let Err(e) = chroot(temp_dir) {
        eprintln!("Failed to chroot: {}", e);
        unsafe { libc::_exit(1) };
    }

    if let Err(e) = std::env::set_current_dir("/") {
        eprintln!("Failed to chdir: {}", e);
        unsafe { libc::_exit(1) };
    }

    // Signal parent that we're ready by closing the sync pipe
    let _ = nix::unistd::close(sync_fd);

    // Stay alive until killed
    loop {
        pause();
    }
}
