extern crate argparse;
extern crate cntr;
extern crate nix;

use clap::{crate_authors, crate_version, Arg, ArgEnum, ArgMatches, Command, PossibleValue};
use cntr::pwnam;
use std::path::Path;
use std::{env, process};

fn command_arg(index: usize) -> Arg<'static> {
    Arg::new("command")
        .help("Command and its arguments to execute after attach. Consider prepending it with '-- ' to prevent parsing of '-x'-like flags. [default: $SHELL]")
        .requires("command")
        .index(index)
        .multiple_occurrences(true)
}

fn parse_command_arg(args: &ArgMatches) -> (Option<String>, Vec<String>) {
    match args.values_of("command") {
        Some(args) => {
            let mut values: Vec<String> = args.map(|s| s.to_string()).collect();
            let command = values.remove(0);
            let command = match command.is_empty() {
                true => None, // indicates $SHELL default case
                false => Some(command),
            };
            let arguments = values;
            (command, arguments)
        }
        None => (None, vec![]), // indicates $SHELL default case
    }
}

#[derive(clap::ArgEnum, Debug, Clone, Copy)]
#[allow(non_camel_case_types)]
pub enum ContainerType {
    process_id,
    rkt,
    podman,
    docker,
    nspawn,
    lxc,
    lxd,
    containerd,
    command,
}

impl ContainerType {
    pub fn possible_values() -> impl Iterator<Item = PossibleValue<'static>> {
        ContainerType::value_variants()
            .iter()
            .filter_map(ArgEnum::to_possible_value)
    }
}

impl std::str::FromStr for ContainerType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        for variant in Self::value_variants() {
            if variant.to_possible_value().unwrap().matches(s, false) {
                return Ok(*variant);
            }
        }
        Err(format!("Invalid variant: {}", s))
    }
}

fn attach(args: &ArgMatches) {
    let (command, arguments) = parse_command_arg(args);

    let container_name = args.value_of("id").unwrap().to_string(); // safe, because container id is .required

    let mut container_types = vec![];
    if args.is_present("type") {
        let types = args
            .values_of_t::<ContainerType>("type")
            .unwrap_or_else(|e| e.exit());
        container_types = types
            .into_iter()
            .filter_map(|t| cntr::lookup_container_type(&format!("{:?}", t)))
            .collect();
    }

    let mut options = cntr::AttachOptions {
        command,
        arguments,
        effective_user: None,
        container_types,
        container_name,
    };

    let effective_username = args.value_of("effective-user").unwrap_or("");
    if !effective_username.is_empty() {
        match pwnam(effective_username) {
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
                    effective_username, e
                );
                process::exit(1);
            }
        };
    }

    if let Err(err) = cntr::attach(&options) {
        eprintln!("{}", err);
        process::exit(1);
    };
}

fn exec(args: &ArgMatches, setcap: bool) {
    let (command, arguments) = parse_command_arg(args);

    if let Err(err) = cntr::exec(command, arguments, setcap) {
        eprintln!("{}", err);
        process::exit(1);
    }
}

fn main() {
    let attach_command = Command::new("attach")
        .about("Enter container")
        .version(crate_version!())
        .author(crate_authors!("\n"))
        .disable_version_flag(true)
        .arg(
            Arg::new("effective-user")
                .long("effective-user")
                .takes_value(true)
                .forbid_empty_values(true)
                .value_name("EFFECTIVE_USER")
                .help("effective username that should be owner of new created files on the host"),
        )
        .arg(
            Arg::new("type")
                .short('t')
                .long("type")
                .takes_value(true)
                .forbid_empty_values(true)
                .require_value_delimiter(true)
                .value_name("TYPE")
                .help("Container types to try (sperated by ','). [default: all but command]")
                .possible_values(ContainerType::possible_values()),
        )
        .arg(
            Arg::new("id")
                .help("container id, container name or process id")
                .required(true)
                .index(1),
        )
        .arg(command_arg(2));

    let exec_command = Command::new("exec")
        .about("Execute command in container filesystem")
        .version(crate_version!())
        .author(crate_authors!("\n"))
        .arg(command_arg(1));

    let main_app = Command::new("Cntr")
        .about("Enter or executed in container")
        .version(crate_version!())
        .author(crate_authors!("\n"))
        .arg_required_else_help(true)
        .subcommand(attach_command)
        .subcommand(exec_command.clone());

    // find and run subcommand/app
    match std::env::current_exe() {
        Ok(exe) => {
            if exe == Path::new(cntr::SETCAP_EXE) {
                let matches = exec_command.get_matches();
                exec(&matches, true);
            } else {
                let matches = main_app.get_matches();
                match matches.subcommand() {
                    Some(("exec", exec_matches)) => exec(exec_matches, false),
                    Some(("attach", attach_matches)) => attach(attach_matches),
                    Some((_, attach_matches)) => attach(attach_matches),
                    None => unreachable!(), // beause of AppSettings::SubCommandRequired
                };
            }
        }
        Err(e) => {
            eprintln!("failed to resolve executable: {}", e);
            process::exit(1);
        }
    }
}
