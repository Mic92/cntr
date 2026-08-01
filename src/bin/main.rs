#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use core::ffi::CStr;
use origin::program;

/// Entry point called by origin after program startup.
///
/// SAFETY: `argv` and `envp` are the NULL-terminated argument and environment
/// arrays provided by the kernel.
#[unsafe(no_mangle)]
unsafe fn origin_main(argc: usize, argv: *mut *mut u8, envp: *mut *mut u8) -> i32 {
    let mut args = Vec::with_capacity(argc);
    for i in 0..argc {
        let arg = unsafe { CStr::from_ptr((*argv.add(i)).cast()) };
        args.push(String::from_utf8_lossy(arg.to_bytes()).into_owned());
    }

    let mut environ = Vec::new();
    let mut env = envp;
    while !unsafe { *env }.is_null() {
        let entry = unsafe { CStr::from_ptr((*env).cast()) }.to_bytes();
        if let Some(pos) = entry.iter().position(|&b| b == b'=') {
            environ.push((entry[..pos].to_vec(), entry[pos + 1..].to_vec()));
        }
        env = unsafe { env.add(1) };
    }

    let status = match cntr::cli::run_with_args(args, environ) {
        Ok(code) => i32::from(code),
        Err(e) => {
            cntr::stderrln!("{}", e);
            1
        }
    };
    program::exit(status)
}

#[cfg(not(test))]
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    cntr::stderrln!("panic: {}", info);
    program::trap()
}

// Referenced by the precompiled alloc rlib; unused with panic = "abort".
#[cfg(not(test))]
#[unsafe(no_mangle)]
extern "C" fn rust_eh_personality() {}

// compiler_builtins' aarch64 outline-atomics helpers are built with stack
// protector enabled; provide the guard symbols normally supplied by libc.
#[cfg(all(not(test), target_arch = "aarch64"))]
#[allow(non_upper_case_globals)]
#[unsafe(no_mangle)]
static __stack_chk_guard: usize = 0xdead_beef_0bad_cafe;

#[cfg(all(not(test), target_arch = "aarch64"))]
#[unsafe(no_mangle)]
extern "C" fn __stack_chk_fail() -> ! {
    program::trap()
}
