use crate::errors::format_chain;
use crate::passwd::{self, User};

use crate::{ApparmorMode, AttachOptions, attach, exec};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const AUTHORS: &str = env!("CARGO_PKG_AUTHORS");

/// Parse container types from comma-separated string
fn parse_container_types(s: &str) -> Result<Vec<Box<dyn crate::container_pid::Container>>, String> {
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
    crate::stderrln!("cntr-attach {}", VERSION);
    crate::stderrln!("by {}", AUTHORS);
    crate::stderrln!();
    crate::stderrln!("USAGE:");
    crate::stderrln!("    cntr attach [OPTIONS] <CONTAINER_ID> [-- <COMMAND>...]");
    crate::stderrln!();
    crate::stderrln!("ARGS:");
    crate::stderrln!("    <CONTAINER_ID>    Container ID, name, or process ID");
    crate::stderrln!();
    crate::stderrln!("OPTIONS:");
    crate::stderrln!("    -t, --type <TYPES>           Container types to try (comma-separated)");
    crate::stderrln!(
        "                                 [possible: process_id,podman,docker,nspawn,lxc,lxd,containerd,command,kubernetes]"
    );
    crate::stderrln!("                                 [default: all but command]");
    crate::stderrln!("    --effective-user <USER>      Effective user for new files on host");
    crate::stderrln!("                                 (username or numeric uid[:gid])");
    crate::stderrln!("    --apparmor <MODE>            AppArmor profile mode");
    crate::stderrln!("                                 [possible: auto, off]");
    crate::stderrln!("                                 [default: auto]");
    crate::stderrln!("    -h, --help                   Print help");
    crate::stderrln!("    -V, --version                Print version");
    crate::stderrln!();
    crate::stderrln!("COMMAND:");
    crate::stderrln!("    Command and arguments to execute [default: $SHELL]");
    crate::stderrln!("    Use '--' to separate command from options");
}

/// Print help for exec command
fn print_exec_help() {
    crate::stderrln!("cntr-exec {}", VERSION);
    crate::stderrln!("by {}", AUTHORS);
    crate::stderrln!();
    crate::stderrln!("USAGE:");
    crate::stderrln!("    cntr exec [OPTIONS] <CONTAINER_ID> [-- <COMMAND>...]");
    crate::stderrln!();
    crate::stderrln!("ARGS:");
    crate::stderrln!("    <CONTAINER_ID>    Container ID, name, or process ID (required)");
    crate::stderrln!();
    crate::stderrln!("OPTIONS:");
    crate::stderrln!("    -t, --type <TYPES>           Container types to try (comma-separated)");
    crate::stderrln!(
        "                                 [possible: process_id,podman,docker,nspawn,lxc,lxd,containerd,command,kubernetes]"
    );
    crate::stderrln!("                                 [default: all but command]");
    crate::stderrln!("    --apparmor <MODE>            AppArmor profile mode");
    crate::stderrln!("                                 [possible: auto, off]");
    crate::stderrln!("                                 [default: auto]");
    crate::stderrln!("    -h, --help                   Print help");
    crate::stderrln!("    -V, --version                Print version");
    crate::stderrln!();
    crate::stderrln!("COMMAND:");
    crate::stderrln!("    Command and arguments to execute [default: /bin/sh]");
    crate::stderrln!("    Use '--' to separate command from options");
}

/// Print main help
fn print_help() {
    crate::stderrln!("cntr {}", VERSION);
    crate::stderrln!("by {}", AUTHORS);
    crate::stderrln!();
    crate::stderrln!("Enter or execute in container");
    crate::stderrln!();
    crate::stderrln!("USAGE:");
    crate::stderrln!("    cntr <SUBCOMMAND>");
    crate::stderrln!();
    crate::stderrln!("SUBCOMMANDS:");
    crate::stderrln!("    attach    Enter container with mount overlay");
    crate::stderrln!("    exec      Execute command in container");
    crate::stderrln!("    help      Print help");
    crate::stderrln!("    version   Print version");
}

/// Parse attach command arguments
fn parse_attach_args<I>(mut args: I) -> Result<std::process::ExitCode, Box<dyn std::error::Error>>
where
    I: Iterator<Item = String>,
{
    let mut container_id: Option<String> = None;
    let mut container_types: Vec<Box<dyn crate::container_pid::Container>> = vec![];
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
                crate::stderrln!("cntr {}", VERSION);
                return Ok(std::process::ExitCode::SUCCESS);
            }
            "-t" | "--type" => {
                let types_str = args.next().ok_or("--type requires an argument")?;
                container_types = parse_container_types(&types_str)
                    .map_err(|e| format!("invalid --type argument '{}': {}", types_str, e))?;
            }
            "--effective-user" => {
                let username = args.next().ok_or("--effective-user requires an argument")?;
                match passwd::lookup(&username) {
                    Ok(Some(user)) => effective_user = Some(user),
                    Ok(None) => {
                        return Err(format!("user '{}' not found", username).into());
                    }
                    Err(e) => {
                        return Err(format!(
                            "failed to lookup user '{}': {}",
                            username,
                            format_chain(&e)
                        )
                        .into());
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

    attach(&options).map_err(|e| {
        format!(
            "failed to attach to container '{}': {}",
            container_name,
            format_chain(&e)
        )
    })?;
    Ok(std::process::ExitCode::SUCCESS)
}

/// Parse exec command arguments
fn parse_exec_args<I>(mut args: I) -> Result<std::process::ExitCode, Box<dyn std::error::Error>>
where
    I: Iterator<Item = String>,
{
    let mut container_id: Option<String> = None;
    let mut container_types: Vec<Box<dyn crate::container_pid::Container>> = vec![];
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
                crate::stderrln!("cntr {}", VERSION);
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

    exec::exec(&options).map_err(|e| {
        format!(
            "failed to exec into container '{}': {}",
            container_name,
            format_chain(&e)
        )
    })?;

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
    if crate::env::var("CNTR_ALLOW_SETCAP") == Some("1") {
        use rustix::process::{DumpableBehavior, set_dumpable_behavior};
        if let Err(e) = set_dumpable_behavior(DumpableBehavior::Dumpable) {
            log::warn!("failed to set PR_SET_DUMPABLE: {}", e);
        }
    }
}

pub fn run_with_args<I, T>(args: I) -> Result<std::process::ExitCode, Box<dyn std::error::Error>>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    crate::env::init();
    crate::logging::init();

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
            crate::stderrln!("cntr {}", VERSION);
            Ok(std::process::ExitCode::SUCCESS)
        }
        _ => Err(format!("unknown subcommand: {}", subcommand).into()),
    }
}
