fn main() {
    // Start via origin's _start, link no libc/libgcc_s, and link statically
    // since there is no dynamic linker without libc.
    println!("cargo:rustc-link-arg-bins=-nostartfiles");
    println!("cargo:rustc-link-arg-bins=-nodefaultlibs");
    println!("cargo:rustc-link-arg-bins=-static");
}
