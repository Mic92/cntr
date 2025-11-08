extern crate cntr;

use std::{env, process};

fn main() {
    if let Err(e) = cntr::cli::run_with_args(env::args_os()) {
        eprintln!("{}", e);
        process::exit(1);
    }
}
