extern crate argparse;
extern crate cntr;
extern crate nix;

use clap::{crate_version, crate_authors, values_t, App, AppSettings, Arg, ArgMatches, SubCommand};
use cntr::pwnam;
use cntr::ContainerType;
use std::{env, process};

fn attach(args: &ArgMatches) {
    let command = args.value_of("command").unwrap(); // always has default value
    let arguments = match args.values_of("arguments") {
        Some(arguments) => arguments.map(|s| s.to_string()).collect(),
        None => vec![],
    };

    let container_name = args.value_of("id").unwrap().to_string(); // container id is .required

    let mut container_types = vec![];
    if args.is_present("type") {
        let types = values_t!(args.values_of("type"), ContainerType).unwrap_or_else(|e| e.exit());
        container_types = types.into_iter().map(|t| cntr::lookup_container_type(&t)).collect();
    }

    let mut options = cntr::AttachOptions {
        command: match command {
            "$SHELL" => None,
            _ => Some(command.to_string()),
        },
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
    let command = args.value_of("command").unwrap(); // always has default value
    let command = if command.is_empty() {
        None
    } else {
        Some(command.to_string())
    };

    let arguments = match args.values_of("arguments") {
        Some(arguments) => arguments.map(|s| s.to_string()).collect(),
        None => vec![],
    };

    if let Err(err) = cntr::exec(command, arguments, setcap) {
        eprintln!("{}", err);
        process::exit(1);
    }
}

fn main() {
    let attach_command = SubCommand::with_name("attach")
        .about("Enter container")
        .setting(AppSettings::DisableVersion)
        .arg(
            Arg::with_name("effective-user")
                .long("effective-user")
                .takes_value(true)
                .empty_values(false)
                .value_name("EFFECTIVE_USER")
                .help("effective username that should be owner of new created files on the host"),
        )
        .arg(
            Arg::with_name("type")
                .short("t")
                .long("type")
                .takes_value(true)
                .empty_values(false)
                .require_delimiter(true)
                .value_name("TYPE")
                .help("Container types to try (sperated by ',') [default: all but command]")
                .possible_values(&ContainerType::variants()),
        )
        .arg(
            Arg::with_name("id")
                .help("container id, container name or process id")
                .required(true)
                .index(1),
        )
        .arg(
            Arg::with_name("command")
                .help("command to execute after attach")
                .default_value("$SHELL")
                .required(true)
                .index(2),
        )
        .arg(
            Arg::with_name("arguments")
                .help("arguments passed to command")
                .requires("command")
                .index(3)
                .multiple(true),
        );

    let exec_command = SubCommand::with_name("exec")
        .about("Execute command in container filesystem")
        .arg(
            Arg::with_name("command")
                .help("command to execute (default: $SHELL)")
                .required(true)
                .index(1),
        )
        .arg(
            Arg::with_name("arguments")
                .help("arguments passed to command")
                .requires("command")
                .index(2)
                .multiple(true),
        );

    let matches = App::new("Cntr")
        .about("Enter or executed in container")
        .version(crate_version!())
        .author(crate_authors!("\n"))
        .setting(AppSettings::SubcommandRequiredElseHelp)
        .subcommand(attach_command)
        .subcommand(exec_command)
        .get_matches();

    match matches.subcommand() {
        ("exec", Some(exec_matches)) => exec(exec_matches, true),
        ("attach", Some(attach_matches)) => attach(attach_matches),
        ("", None) => unreachable!(), // beause of AppSettings::SubCommandRequired
        _ => unreachable!(),
    };
}
