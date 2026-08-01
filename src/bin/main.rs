use std::os::unix::ffi::OsStringExt;
use std::process;

fn main() -> process::ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let environ: Vec<(Vec<u8>, Vec<u8>)> = std::env::vars_os()
        .map(|(key, value)| (key.into_vec(), value.into_vec()))
        .collect();
    match cntr::cli::run_with_args(args, environ) {
        Ok(code) => process::ExitCode::from(code),
        Err(e) => {
            cntr::stderrln!("{}", e);
            process::ExitCode::FAILURE
        }
    }
}
