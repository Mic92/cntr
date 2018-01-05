extern crate argparse;
extern crate cntr;
extern crate nix;

use argparse::{ArgumentParser, Store};
use cntr::pwd::pwnam;
use std::process;

fn parse_args() -> cntr::Options {
    let mut options = cntr::Options {
        container_name: String::from(""),
        container_types: vec![],
        effective_user: None,
    };
    let mut container_type = String::from("");
    let mut container_name = String::from("");
    let mut effective_username = String::from("");
    {
        let mut ap = ArgumentParser::new();
        ap.set_description("Enter container");
        ap.refer(&mut effective_username).add_option(
            &["--effective-user"],
            Store,
            "effective username that should be owner of new created files on the host",
        );
        ap.refer(&mut container_type).add_option(
            &["--type"],
            Store,
            "Container type (docker|generic)",
        );
        ap.refer(&mut container_name).add_argument(
            "id",
            Store,
            "container id, container name or process id",
        );
        ap.parse_args_or_exit();
    }
    options.container_name = container_name;

    if !container_type.is_empty() {
        options.container_types = match cntr::lookup_container_type(container_type.as_str()) {
            Some(container) => vec![container],
            None => {
                eprintln!(
                    "invalid argument '{}' passed to `--type`; valid values are: {}",
                    container_type,
                    cntr::AVAILABLE_CONTAINER_TYPES.join(", ")
                );
                process::exit(1)
            }
        };
    }

    if effective_username != "" {
        match pwnam(effective_username.as_str()) {
            Ok(Some(passwd)) => {
                options.effective_user = Some(passwd);
            }
            Ok(None) => {
                eprintln!("no user with username '{}' found", effective_username);
                process::exit(1);
            }
            Err(e) => {
                eprintln!(
                    "failed to to lookup user '{}' found: {}",
                    effective_username,
                    e
                );
                process::exit(1);
            }
        };
    }

    options
}

fn main() {
    let opts = parse_args();
    if let Err(err) = cntr::run(&opts) {
        eprintln!("{}", err);
        process::exit(1);
    };
}
