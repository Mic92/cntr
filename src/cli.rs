use nix::unistd::User;
use std::env;

use crate::{ApparmorMode, AttachOptions, attach, exec};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const AUTHORS: &str = env!("CARGO_PKG_AUTHORS");

/// Parse container types from comma-separated string
fn parse_container_types(s: &str) -> Result<Vec<Box<dyn container_pid::Container>>, String> {
    let mut valid_types = Vec::new();
    let mut unknown_names = Vec::new();

    for token in s.split(',') {
        let trimmed = token.trim();
        if let Some(container_type) = crate::lookup_container_type(trimmed) {
            valid_types.push(container_type);
        } else {
            unknown_names.push(trimmed.to_string());
        }
    }

    if !unknown_names.is_empty() {
        return Err(format!(
            "unknown container type(s): {}",
            unknown_names.join(", ")
        ));
    }

    Ok(valid_types)
}

/// Parse AppArmor mode from string
fn parse_apparmor_mode(s: &str) -> Result<ApparmorMode, String> {
    match s.to_lowercase().as_str() {
        "auto" => Ok(ApparmorMode::Auto),
        "off" => Ok(ApparmorMode::Off),
        _ => Err(format!(
            "invalid apparmor mode '{}', expected 'auto' or 'off'",
            s
        )),
    }
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
        "                                 [possible: process_id,podman,docker,nspawn,lxc,lxd,containerd,command,kubernetes]"
    );
    eprintln!("                                 [default: all but command]");
    eprintln!("    --effective-user <USER>      Effective username for new files on host");
    eprintln!("    --apparmor <MODE>            AppArmor profile mode");
    eprintln!("                                 [possible: auto, off]");
    eprintln!("                                 [default: auto]");
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
        "                                 [possible: process_id,podman,docker,nspawn,lxc,lxd,containerd,command,kubernetes]"
    );
    eprintln!("                                 [default: all but command]");
    eprintln!("    --apparmor <MODE>            AppArmor profile mode");
    eprintln!("                                 [possible: auto, off]");
    eprintln!("                                 [default: auto]");
    eprintln!("    -h, --help                   Print help");
    eprintln!("    -V, --version                Print version");
    eprintln!();
    eprintln!("COMMAND:");
    eprintln!("    Command and arguments to execute [default: /bin/sh]");
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
fn parse_attach_args<I>(mut args: I) -> Result<std::process::ExitCode, Box<dyn std::error::Error>>
where
    I: Iterator<Item = String>,
{
    let mut container_id: Option<String> = None;
    let mut container_types: Vec<Box<dyn container_pid::Container>> = vec![];
    let mut effective_user: Option<User> = None;
    let mut apparmor_mode = ApparmorMode::Auto;
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
                return Ok(std::process::ExitCode::SUCCESS);
            }
            "-V" | "--version" => {
                eprintln!("cntr {}", VERSION);
                return Ok(std::process::ExitCode::SUCCESS);
            }
            "-t" | "--type" => {
                let types_str = args.next().ok_or("--type requires an argument")?;
                container_types = parse_container_types(&types_str)
                    .map_err(|e| format!("invalid --type argument '{}': {}", types_str, e))?;
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
            "--apparmor" => {
                let mode_str = args.next().ok_or("--apparmor requires an argument")?;
                apparmor_mode = parse_apparmor_mode(&mode_str).map_err(|e| e.to_string())?;
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
        container_name: container_name.clone(),
        container_types,
        effective_user,
        apparmor_mode,
    };

    attach(&options)
        .map_err(|e| format!("failed to attach to container '{}': {}", container_name, e))?;
    Ok(std::process::ExitCode::SUCCESS)
}

/// Parse exec command arguments
fn parse_exec_args<I>(mut args: I) -> Result<std::process::ExitCode, Box<dyn std::error::Error>>
where
    I: Iterator<Item = String>,
{
    let mut container_id: Option<String> = None;
    let mut container_types: Vec<Box<dyn container_pid::Container>> = vec![];
    let mut apparmor_mode = ApparmorMode::Auto;
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
                return Ok(std::process::ExitCode::SUCCESS);
            }
            "-V" | "--version" => {
                eprintln!("cntr {}", VERSION);
                return Ok(std::process::ExitCode::SUCCESS);
            }
            "-t" | "--type" => {
                let types_str = args.next().ok_or("--type requires an argument")?;
                container_types = parse_container_types(&types_str)
                    .map_err(|e| format!("invalid --type argument '{}': {}", types_str, e))?;
            }
            "--apparmor" => {
                let mode_str = args.next().ok_or("--apparmor requires an argument")?;
                apparmor_mode = parse_apparmor_mode(&mode_str).map_err(|e| e.to_string())?;
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

    let options = exec::ExecOptions {
        command,
        arguments,
        container_name: container_name.clone(),
        container_types,
        apparmor_mode,
    };

    exec::exec(&options)
        .map_err(|e| format!("failed to exec into container '{}': {}", container_name, e))?;

    Ok(std::process::ExitCode::SUCCESS)
}

/// Enable dumpable mode if CNTR_ALLOW_SETCAP=1 is set.
///
/// When running cntr with file capabilities (setcap), the process becomes
/// non-dumpable which prevents access to /proc/self/ns. Setting dumpable=1
/// re-enables this access but has security implications:
/// - Core dumps may expose privileged memory
/// - Other processes running as the same user can ptrace this process
///
/// Only enable this if you understand the security tradeoffs.
fn maybe_set_dumpable() {
    if env::var("CNTR_ALLOW_SETCAP").as_deref() == Ok("1") {
        use crate::syscalls::prctl::prctl;
        // PR_SET_DUMPABLE = 4, SUID_DUMP_USER = 1
        if let Err(e) = prctl(4, 1, 0, 0, 0) {
            log::warn!("failed to set PR_SET_DUMPABLE: {}", e);
        }
    }
}

pub fn run_with_args<I, T>(args: I) -> Result<std::process::ExitCode, Box<dyn std::error::Error>>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    // Must be called early, before any /proc/self access
    maybe_set_dumpable();

    let args: Vec<String> = args
        .into_iter()
        .map(|s| {
            let os_string: std::ffi::OsString = s.into();
            os_string.into_string().map_err(|invalid| {
                format!(
                    "argument contains invalid UTF-8: {}",
                    invalid.to_string_lossy()
                )
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

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
            Ok(std::process::ExitCode::SUCCESS)
        }
        "version" | "-V" | "--version" => {
            eprintln!("cntr {}", VERSION);
            Ok(std::process::ExitCode::SUCCESS)
        }
        _ => Err(format!("unknown subcommand: {}", subcommand).into()),
    }
}
