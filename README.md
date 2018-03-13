# cntr

[![Build Status](https://travis-ci.org/Mic92/cntr.svg?branch=master)](https://travis-ci.org/Mic92/cntr)

Say no to `$ apt install vim` in containers!

Cntr is a tool that allows to attach you to container from your host. It allows
to users to use their favorite debugging tools (tcpdump, curl, htop, strace,
rg/ag, shell + dotfiles, $EDITOR), installed on the host within the container.
Therefore it spawns a shell that inherits the full context of the
container and mount itself as a fuse filesystem.

## Demo

- TODO: ascii cinema

## Features

- supports the following container engines natively:
  * docker
  * LXC
  * rkt
  * systemd-nspawn
- Additional all other containers/sandboxes should work too, however the process
  id has to be provided instead of container names/ids
- the following container properties are inherited:
  * namespaces (mount, uts, pid, net, cgroup, ipc)
  * cgroups
  * apparamor/selinux
  * capabilities
  * user/group ids
  * environment variables
  * the following files: /etc/passwd, /etc/hostname, /etc/hosts, /etc/resolv.conf

## Usage

```console
$ cntr --help
Usage:
    ./target/debug/cntr COMMAND [ARGUMENTS ...]
Enter or executed in container
positional arguments:
  command               Command to run (either "attach" or "exec")
  arguments             Arguments for command
optional arguments:
  -h,--help             show this help message and exit
```

```console
$ ./target/debug/cntr attach --help
Usage:
    subcommand attach [OPTIONS] ID [COMMAND] [ARGUMENTS ...]
Enter container
positional arguments:
  id                    container id, container name or process id
  command               command to execute after attach (default: $SHELL)
  arguments             arguments passed to command
optional arguments:
  -h,--help             show this help message and exit
  --effective-user EFFECTIVE_USER
                        effective username that should be owner of new created
                        files on the host
  --type TYPE           Container type (docker|lxc|rkt|process_id|nspawn,
                        default: all)
```

```console
$ cntr exec --help
Usage:
    subcommand exec [COMMAND] [ARGUMENTS ...]
Execute command in container filesystem
positional arguments:
  command               command to execute (default: $SHELL)
  arguments             Arguments to pass to command
optional arguments:
  -h,--help             show this help message and exit
```

### Docker

```
$ docker run --name boxbusy -ti busybox
$ docker ps
CONTAINER ID        IMAGE               COMMAND             CREATED             STATUS              PORTS               NAMES
55a93d71b53b        busybox             "sh"                22 seconds ago      Up 20 seconds                           boxbusy
```

Either provide a container id...

```console
$ cntr attach 55a93d71b53b
[root@55a93d71b53b:/var/lib/cntr]# echo "I am in a container!"
[root@55a93d71b53b:/var/lib/cntr]# ip addr
1: lo: <LOOPBACK,UP,LOWER_UP> mtu 65536 qdisc noqueue state UNKNOWN group default qlen 1000
    link/loopback 00:00:00:00:00:00 brd 00:00:00:00:00:00
    inet 127.0.0.1/8 scope host lo
       valid_lft forever preferred_lft forever
40: eth0@if41: <BROADCAST,MULTICAST,UP,LOWER_UP> mtu 1500 qdisc noqueue state UP group default
    link/ether 02:42:ac:11:00:02 brd ff:ff:ff:ff:ff:ff link-netnsid 0
    inet 172.17.0.2/16 brd 172.17.255.255 scope global eth0
       valid_lft forever preferred_lft forever
[root@55a93d71b53b:/var/lib/cntr]# vim etc/resolv.conf
```

...or the container name.
Use `cntr exec` to execute container native commands (while running in the cntr shell).

```console
$ cntr attach boxbusy
[root@55a93d71b53b:/var/lib/cntr]# cntr exec sh -c 'busybox | head -1'
```

### LXC

...

### rkt

...

### systemd-nspawn

...

### Generic process id

...

## Installing

### Pre-build static-linked binary

For linux x86_64 we build static binaries for every release. More platforms can added on request.
See the [release tab](https://github.com/Mic92/cntr/releases/download/1.0-beta/cntr-1.0-beta-x86_64-unknown-linux-musl.tar.gz) for pre-build tarballs.

### Build from source

```console
$ cargo install --git https://github.com/Mic92/cntr
```
