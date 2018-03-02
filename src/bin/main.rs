extern crate argparse;
extern crate cntr;
extern crate nix;

use argparse::{ArgumentParser, Store, List};
use cntr::pwd::pwnam;
use std::io::{stdout, stderr};
use std::process;
use std::str::FromStr;

#[allow(non_camel_case_types)]
#[derive(Debug)]
enum Command {
    attach,
    exec,
}

impl FromStr for Command {
    type Err = ();
    fn from_str(src: &str) -> Result<Command, ()> {
        return match src {
            "attach" => Ok(Command::attach),
            "exec" => Ok(Command::exec),
            _ => Err(()),
        };
    }
}

fn parse_attach_args(args: Vec<String>) -> cntr::AttachOptions {
    let mut options = cntr::AttachOptions {
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
        match ap.parse(args, &mut stdout(), &mut stderr()) {
            Ok(()) =>  {}
            Err(x) => {
                std::process::exit(x);
            }
        }
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

fn attach_command(args: Vec<String>) {
    let opts = parse_attach_args(args);
    if let Err(err) = cntr::attach(&opts) {
        eprintln!("{}", err);
        process::exit(1);
    };
}

fn exec_command(args: Vec<String>) {
    if let Err(err) = cntr::exec(&args[1], &args[2..]) {
        eprintln!("{}", err);
        process::exit(1);
    }
}

fn main() {
    let mut subcommand = Command::attach;
    let mut args = vec![];
    {
        let mut ap = ArgumentParser::new();
        ap.set_description("Enter or executed in container");
        ap.refer(&mut subcommand).add_argument(
            "command",
            Store,
            r#"Command to run (either "attach" or "exec")"#,
        );
        ap.refer(&mut args).add_argument(
            "arguments",
            List,
            r#"Arguments for command"#,
        );

        ap.parse_args_or_exit();
    }

    args.insert(0, format!("subcommand {:?}", subcommand));

    match subcommand {
        Command::attach => attach_command(args),
        Command::exec => exec_command(args),
    }
}
