fn main() {
    println!("cargo::rustc-check-cfg=cfg(origin_start)");
    // origin only has start code for these architectures; elsewhere the
    // binary falls back to a regular libc/std start.
    let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap();
    if matches!(arch.as_str(), "x86_64" | "aarch64" | "x86" | "riscv64") {
        println!("cargo:rustc-cfg=origin_start");
        // Start via origin's _start, link no libc/libgcc_s, and link statically
        // since there is no dynamic linker without libc.
        println!("cargo:rustc-link-arg-bins=-nostartfiles");
        println!("cargo:rustc-link-arg-bins=-nodefaultlibs");
        println!("cargo:rustc-link-arg-bins=-static");
    }
}
