extern crate argparse;
extern crate cntr;
extern crate nix;

use clap::{crate_authors, crate_version, values_t, App, AppSettings, Arg, ArgMatches, SubCommand};
use cntr::pwnam;
use cntr::ContainerType;
use std::{env, process};

fn command_arg(index: u64) -> Arg<'static, 'static> {
    Arg::with_name("command")
                .help("Command and its arguments to execute after attach. Consider prepending it with '-- ' to prevent parsing of '-x'-like flags. [default: $SHELL]")
                .requires("command")
                .index(index)
                .multiple(true)
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

fn attach(args: &ArgMatches) {
    let (command, arguments) = parse_command_arg(args);

    let container_name = args.value_of("id").unwrap().to_string(); // safe, because container id is .required

    let mut container_types = vec![];
    if args.is_present("type") {
        let types = values_t!(args.values_of("type"), ContainerType).unwrap_or_else(|e| e.exit());
        container_types = types
            .into_iter()
            .map(|t| cntr::lookup_container_type(&t))
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
                .help("Container types to try (sperated by ','). [default: all but command]")
                .possible_values(&ContainerType::variants()),
        )
        .arg(
            Arg::with_name("id")
                .help("container id, container name or process id")
                .required(true)
                .index(1),
        )
        .arg(command_arg(2));

    let exec_command = SubCommand::with_name("exec")
        .about("Execute command in container filesystem")
        .arg(command_arg(1));

    let matches = App::new("Cntr")
        .about("Enter or executed in container")
        .version(crate_version!())
        .author(crate_authors!("\n"))
        .setting(AppSettings::SubcommandRequiredElseHelp)
        .setting(AppSettings::VersionlessSubcommands)
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
