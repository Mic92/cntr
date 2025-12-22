cntr(1)

# NAME

cntr - a container debugging tool based on Linux mount API

# SYNOPSIS

*cntr* attach [_OPTIONS_] _CONTAINER_ID_ [-- _COMMAND_...]++
*cntr* exec [_OPTIONS_] _CONTAINER_ID_ [-- _COMMAND_...]++
*cntr* help++
*cntr* version

# DESCRIPTION

cntr is a replacement for *docker exec* and similar commands that allows you to
attach to a running container with your own debugging tools. Instead of
installing debug tools inside the container, cntr mounts your host filesystem
into the container's mount namespace, giving you access to all your tools while
running inside the container.

# SUBCOMMANDS

*attach* [_OPTIONS_] _CONTAINER_ID_ [-- _COMMAND_...]
	Enter a container with a mount overlay. The host filesystem is mounted at
	*/var/lib/cntr* inside the container, and host binaries are available via
	a modified *PATH*. If no command is specified, *$SHELL* is executed.

*exec* [_OPTIONS_] _CONTAINER_ID_ [-- _COMMAND_...]
	Execute a command in a container without the mount overlay. This is similar
	to *docker exec* but supports multiple container runtimes. If no command
	is specified, */bin/sh* is executed.

*help*
	Print help information.

*version*
	Print version information.

# OPTIONS

## Common Options

*-t*, *--type* _TYPES_
	Container types to try, as a comma-separated list. If not specified, all
	types except *command* are tried.

	Possible values:
	- *process_id*: Direct process ID
	- *podman*: Podman containers
	- *docker*: Docker containers
	- *nspawn*: systemd-nspawn containers
	- *lxc*: LXC containers
	- *lxd*: LXD containers
	- *containerd*: containerd containers
	- *command*: Execute a command to get the PID
	- *kubernetes*: Kubernetes containers

*--apparmor* _MODE_
	AppArmor profile mode. Default: *auto*.

	Possible values:
	- *auto*: Automatically handle AppArmor profiles
	- *off*: Disable AppArmor profile transitions

*-h*, *--help*
	Print help information.

*-V*, *--version*
	Print version information.

## Attach-specific Options

*--effective-user* _USER_
	Set the effective username for new files created on the host filesystem.
	This affects file ownership when creating files via the mount overlay.

# EXAMPLES

Attach to a Docker container with an interactive shell:

	cntr attach my-container

Attach to a Podman container and run a specific command:

	cntr attach --type podman my-container -- htop

Execute a command in a container by process ID:

	cntr exec --type process_id 12345 -- ps aux

Attach to a systemd-nspawn container:

	cntr attach --type nspawn my-machine

# ENVIRONMENT

*SHELL*
	Used as the default command for *cntr attach* when no command is specified.

# REQUIREMENTS

cntr requires Linux kernel 6.8 or later with mount API support.

# SEE ALSO

*docker-exec*(1), *podman-exec*(1), *machinectl*(1), *lxc-attach*(1), *nsenter*(1)

# AUTHORS

Jörg Thalheim <joerg@thalheim.io>

# BUGS

Report bugs at https://github.com/Mic92/cntr/issues
