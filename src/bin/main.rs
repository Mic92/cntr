extern crate cntr;

use std::{env, process};

fn main() -> process::ExitCode {
    match cntr::cli::run_with_args(env::args_os()) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("{}", e);
            process::ExitCode::FAILURE
        }
    }
}
