extern crate argparse;
extern crate cntr;
extern crate nix;

use argparse::{ArgumentParser, Store};
use std::process;
use cntr::container::ContainerType;

fn parse_args() -> cntr::Options {
    let mut options = cntr::Options {
        container_name: String::from(""),
        container_type: None,
    };
    let mut container_type = String::from("");
    let mut container_name = String::from("");
    {
        let mut ap = ArgumentParser::new();
        ap.set_description("Enter container");
        ap.refer(&mut container_type).add_option(&["--type"], Store, "Container type (docker|generic)");
        ap.refer(&mut container_name).add_argument("id", Store, "container id, container name or process id");
        ap.parse_args_or_exit();
    }
    options.container_name = container_name;
    options.container_type = match container_type.as_str() {
        "docker" => Some(ContainerType::Docker),
        "pid" => Some(ContainerType::ProcessId),
        "" => None,
        _ => {
            eprintln!("invalid argument '{}' passed to `--type`; valid values are: docker, pid", container_type);
            process::exit(1);
        }
    };

    options
}

fn main() {
    let opts = parse_args();
    if let Err(err) = cntr::run(&opts) {
        eprintln!("{}", err);
        process::exit(1);
    };
}
