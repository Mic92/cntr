use nix::unistd::User;
use std::env;

use crate::{AttachOptions, attach, exec};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const AUTHORS: &str = env!("CARGO_PKG_AUTHORS");

/// Parse container types from comma-separated string
fn parse_container_types(s: &str) -> Vec<Box<dyn container_pid::Container>> {
    s.split(',')
        .filter_map(|t| crate::lookup_container_type(t.trim()))
        .collect()
}

/// Print help for attach command
fn print_attach_help() {
    eprintln!("cntr-attach {}", VERSION);
    eprintln!("by {}", AUTHORS);
    eprintln!();
    eprintln!("USAGE:");
    eprintln!("    cntr attach [OPTIONS] <CONTAINER_ID> [-- <COMMAND>...]");
    eprintln!();
    eprintln!("ARGS:");
    eprintln!("    <CONTAINER_ID>    Container ID, name, or process ID");
    eprintln!();
    eprintln!("OPTIONS:");
    eprintln!("    -t, --type <TYPES>           Container types to try (comma-separated)");
    eprintln!(
        "                                 [possible: process-id,podman,docker,nspawn,lxc,lxd,containerd,command,kubernetes]"
    );
    eprintln!("                                 [default: all but command]");
    eprintln!("    --effective-user <USER>      Effective username for new files on host");
    eprintln!("    -h, --help                   Print help");
    eprintln!("    -V, --version                Print version");
    eprintln!();
    eprintln!("COMMAND:");
    eprintln!("    Command and arguments to execute [default: $SHELL]");
    eprintln!("    Use '--' to separate command from options");
}

/// Print help for exec command
fn print_exec_help() {
    eprintln!("cntr-exec {}", VERSION);
    eprintln!("by {}", AUTHORS);
    eprintln!();
    eprintln!("USAGE:");
    eprintln!("    cntr exec [OPTIONS] <CONTAINER_ID> [-- <COMMAND>...]");
    eprintln!();
    eprintln!("ARGS:");
    eprintln!("    <CONTAINER_ID>    Container ID, name, or process ID (required)");
    eprintln!();
    eprintln!("OPTIONS:");
    eprintln!("    -t, --type <TYPES>           Container types to try (comma-separated)");
    eprintln!(
        "                                 [possible: process-id,podman,docker,nspawn,lxc,lxd,containerd,command,kubernetes]"
    );
    eprintln!("                                 [default: all but command]");
    eprintln!("    -h, --help                   Print help");
    eprintln!("    -V, --version                Print version");
    eprintln!();
    eprintln!("COMMAND:");
    eprintln!("    Command and arguments to execute [default: $SHELL]");
    eprintln!("    Use '--' to separate command from options");
}

/// Print main help
fn print_help() {
    eprintln!("cntr {}", VERSION);
    eprintln!("by {}", AUTHORS);
    eprintln!();
    eprintln!("Enter or execute in container");
    eprintln!();
    eprintln!("USAGE:");
    eprintln!("    cntr <SUBCOMMAND>");
    eprintln!();
    eprintln!("SUBCOMMANDS:");
    eprintln!("    attach    Enter container with mount overlay");
    eprintln!("    exec      Execute command in container");
    eprintln!("    help      Print help");
    eprintln!("    version   Print version");
}

/// Parse attach command arguments
fn parse_attach_args<I>(mut args: I) -> Result<(), Box<dyn std::error::Error>>
where
    I: Iterator<Item = String>,
{
    let mut container_id: Option<String> = None;
    let mut container_types: Vec<Box<dyn container_pid::Container>> = vec![];
    let mut effective_user: Option<User> = None;
    let mut command_parts: Vec<String> = vec![];
    let mut in_command = false;

    while let Some(arg) = args.next() {
        if in_command {
            command_parts.push(arg);
            continue;
        }

        match arg.as_str() {
            "-h" | "--help" => {
                print_attach_help();
                std::process::exit(0);
            }
            "-V" | "--version" => {
                eprintln!("cntr {}", VERSION);
                std::process::exit(0);
            }
            "-t" | "--type" => {
                let types_str = args.next().ok_or("--type requires an argument")?;
                container_types = parse_container_types(&types_str);
            }
            "--effective-user" => {
                let username = args.next().ok_or("--effective-user requires an argument")?;
                match User::from_name(&username) {
                    Ok(Some(user)) => effective_user = Some(user),
                    Ok(None) => return Err(format!("user '{}' not found", username).into()),
                    Err(e) => {
                        return Err(format!("failed to lookup user '{}': {}", username, e).into());
                    }
                }
            }
            "--" => {
                in_command = true;
            }
            _ if arg.starts_with('-') => {
                return Err(format!("unknown option: {}", arg).into());
            }
            _ => {
                if container_id.is_none() {
                    container_id = Some(arg);
                } else {
                    // Start of command without '--'
                    command_parts.push(arg);
                    in_command = true;
                }
            }
        }
    }

    let container_name = container_id.ok_or("missing required argument: <CONTAINER_ID>")?;

    let (command, arguments) = if command_parts.is_empty() {
        (None, vec![])
    } else {
        let mut parts = command_parts;
        let cmd = parts.remove(0);
        (Some(cmd), parts)
    };

    let options = AttachOptions {
        command,
        arguments,
        container_name,
        container_types,
        effective_user,
    };

    attach(&options)?;
    Ok(())
}

/// Parse exec command arguments
fn parse_exec_args<I>(mut args: I) -> Result<(), Box<dyn std::error::Error>>
where
    I: Iterator<Item = String>,
{
    let mut container_id: Option<String> = None;
    let mut container_types: Vec<Box<dyn container_pid::Container>> = vec![];
    let mut command_parts: Vec<String> = vec![];
    let mut in_command = false;

    while let Some(arg) = args.next() {
        if in_command {
            command_parts.push(arg);
            continue;
        }

        match arg.as_str() {
            "-h" | "--help" => {
                print_exec_help();
                std::process::exit(0);
            }
            "-V" | "--version" => {
                eprintln!("cntr {}", VERSION);
                std::process::exit(0);
            }
            "-t" | "--type" => {
                let types_str = args.next().ok_or("--type requires an argument")?;
                container_types = parse_container_types(&types_str);
            }
            "--" => {
                in_command = true;
            }
            _ if arg.starts_with('-') => {
                return Err(format!("unknown option: {}", arg).into());
            }
            _ => {
                if container_id.is_none() {
                    container_id = Some(arg);
                } else {
                    // Start of command without '--'
                    command_parts.push(arg);
                    in_command = true;
                }
            }
        }
    }

    let (command, arguments) = if command_parts.is_empty() {
        (None, vec![])
    } else {
        let mut parts = command_parts;
        let cmd = parts.remove(0);
        (Some(cmd), parts)
    };

    // Container ID is now required
    let container_name = container_id.ok_or("container ID is required for exec")?;

    exec::exec(&container_name, &container_types, command, arguments)?;

    Ok(())
}

pub fn run_with_args<I, T>(args: I) -> Result<(), Box<dyn std::error::Error>>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    let args: Vec<String> = args
        .into_iter()
        .map(|s| s.into().into_string().unwrap())
        .collect();

    let mut args_iter = args.into_iter();

    // Skip program name
    let _prog = args_iter.next();

    let subcommand = match args_iter.next() {
        Some(cmd) => cmd,
        None => {
            print_help();
            return Err("no subcommand provided".into());
        }
    };

    match subcommand.as_str() {
        "attach" => parse_attach_args(args_iter),
        "exec" => parse_exec_args(args_iter),
        "help" | "-h" | "--help" => {
            print_help();
            Ok(())
        }
        "version" | "-V" | "--version" => {
            eprintln!("cntr {}", VERSION);
            Ok(())
        }
        _ => Err(format!("unknown subcommand: {}", subcommand).into()),
    }
}
