# cntr

[![Build Status](https://travis-ci.org/Mic92/cntr.svg?branch=master)](https://travis-ci.org/Mic92/cntr)

Cntr is a tool that allows to attach you to container from your host.
It allows to all users to use their favorite debugging tools in the container
they have have installed on the host.
It therefore spawns a shell that inherits the full context of the container and
mount itself as a fuse filesystem.

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

## Usage

### Docker

```
$ docker run --name boxbusy -ti busybox
$ docker ps
CONTAINER ID        IMAGE               COMMAND             CREATED             STATUS              PORTS               NAMES
55a93d71b53b        busybox             "sh"                22 seconds ago      Up 20 seconds                           boxbusy
```

Either provide a container id:

```console
$ cntr attach 55a93d71b53b
[root@55a93d71b53b:/var/lib/cntr]#
```

or the container name:

```console
$ cntr attach boxbusy
[root@55a93d71b53b:/var/lib/cntr]#
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
