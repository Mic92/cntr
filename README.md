# cntr

Say no to `$ apt install vim` in containers!
`cntr` is a replacement for `docker exec` that brings all your developers tools with you.
This is done by mounting the file system from one container or the host into the target container
by creating a nested container with the help of a FUSE filesystem.
This allows to ship minimal runtime image in production and limit the surface for exploits.

Cntr was also published in [Usenix ATC 2018](https://www.usenix.org/conference/atc18/presentation/thalheim).
See [bibtex](#bibtex) for citation.

## Demo

In this two minute recording you learn all the basics of cntr:

[![asciicast](https://asciinema.org/a/PQB5RnRGLIPn88a1R2MtPccdy.png)](https://asciinema.org/a/PQB5RnRGLIPn88a1R2MtPccdy)

## Features

- For convenience cntr supports container names/identifier for the following container engines natively:
  * docker
  * podman
  * LXC
  * LXD
  * rkt
  * systemd-nspawn
  * containerd
- For other container engines cntr also takes process ids (PIDs) instead of container names.

## Installation

Cntr can be only supports linux.

### Pre-build static-linked binary

For linux x86_64 we build static binaries for every release. More platforms can added on request.
See the [release tab](https://github.com/Mic92/cntr/releases/) for pre-build tarballs.
At runtime only commandline utils of the container engine in questions are required.

### Build from source

All you need for compilation is rust + cargo.
Checkout [rustup.rs](https://rustup.rs/) on how to get a working rust toolchain.
Then run:

Either:

```console
$ cargo install cntr
```

Or the latest master:

```console
$ cargo install --git https://github.com/Mic92/cntr
```

For offline builds we also provided a tarball with all dependencies bundled
[here](https://github.com/Mic92/cntr/releases) for compilation with
[cargo-vendor](https://github.com/alexcrichton/cargo-vendor).

## Usage

At a high-level cntr provides two subcommands: `attach` and `exec`:

- `attach`: Allows you to attach to a container with your own native shell/commands.
  Cntr will mount the container at `/var/lib/cntr`.
  The container itself will run unaffected as the mount changes are not visible to container processes.
  - Example: `cntr attach <container_id>` where `container_id` can be a
    container identifier or process id (see examples below).
- `exec`: Once you are in the container, you can also run commands from the
  container filesystem itself. Since those might need their native mount layout
  at `/` instead of `/var/lib/cntr`, cntr provides `exec` subcommand to chroot to container
  again and also resets the environment variables that might have been changed
  by the shell.
  - Example: `cntr exec <command>` where `command` is an executable in the container

**Note**: Cntr needs to run on the same host as the container. It does not work
if the container is running in a virtual machine while cntr is running on the
hypervisor.

```console
$ cntr --help
Cntr 1.5.1
Jörg Thalheim <joerg@thalheim.io>
Enter or executed in container

USAGE:
    cntr <SUBCOMMAND>

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

SUBCOMMANDS:
    attach    Enter container
    exec      Execute command in container filesystem
    help      Prints this message or the help of the given subcommand(s)
```

```console
$ cntr attach --help
cntr-attach 1.5.1
Jörg Thalheim <joerg@thalheim.io>
Enter container

USAGE:
    cntr attach [OPTIONS] <id> [command]...

FLAGS:
    -h, --help    Prints help information

OPTIONS:
        --effective-user <EFFECTIVE_USER>    effective username that should be owner of new created files on the host
    -t, --type <TYPE>                        Container types to try (sperated by ','). [default: all but command]
                                             [possible values: process_id, rkt, podman, docker, nspawn, lxc, lxd,
                                             containerd, command]

ARGS:
    <id>            container id, container name or process id
    <command>...    Command and its arguments to execute after attach. Consider prepending it with '-- ' to prevent
                    parsing of '-x'-like flags. [default: $SHELL]
```

```console
$ cntr exec --help
cntr-exec 1.5.1
Jörg Thalheim <joerg@thalheim.io>
Execute command in container filesystem

USAGE:
    cntr exec [command]...

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

ARGS:
    <command>...    Command and its arguments to execute after attach. Consider prepending it with '-- ' to prevent
                    parsing of '-x'-like flags. [default: $SHELL]
```

### Docker

1: Find out the container name/container id:
```console
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
[root@55a93d71b53b:/var/lib/cntr]# cntr exec -- sh -c 'busybox | head -1'
```

You can also use Dockerfile from this repo to build a docker container with cntr:

``` console
$ docker build -f Dockerfile . -t cntr
# boxbusy here is the name of the target container to attach to
$ docker run --pid=host --privileged=true -v /var/run/docker.sock:/var/run/docker.sock -ti --rm cntr attach boxbusy /bin/sh
```

### Podman

See docker usage, just replace `docker` with the `podman` command.


### LXD

1: Create a container and start it

```console
$ lxc image import images:/alpine/edge
$ lxc launch images:alpine/edge
$ lxc list
+-----------------+---------+------+------+------------+-----------+
|      NAME       |  STATE  | IPV4 | IPV6 |    TYPE    | SNAPSHOTS |
+-----------------+---------+------+------+------------+-----------+
| amazed-sailfish | RUNNING |      |      | PERSISTENT | 0         |
+-----------------+---------+------+------+------------+-----------+
```

2: Attach to the container with cntr

```console
$ cntr attach amazed-sailfish
$ cat etc/hostname
amazed-sailfish
```

### LXC

1: Create a container and start it

```console
$ lxc-create --name ubuntu -t download -- -d ubuntu -r xenial -a amd64
$ lxc-start --name ubuntu -F
...
Ubuntu 16.04.4 LTS ubuntu console
ubuntu login:
$ lxc-ls
ubuntu
```

2: Attach to container with cntr:

```console
$ cntr attach ubuntu
[root@ubuntu2:/var/lib/cntr]# cat etc/os-release
NAME="Ubuntu"
VERSION="16.04.4 LTS (Xenial Xerus)"
ID=ubuntu
ID_LIKE=debian
PRETTY_NAME="Ubuntu 16.04.4 LTS"
VERSION_ID="16.04"
HOME_URL="http://www.ubuntu.com/"
SUPPORT_URL="http://help.ubuntu.com/"
BUG_REPORT_URL="http://bugs.launchpad.net/ubuntu/"
VERSION_CODENAME=xenial
UBUNTU_CODENAME=xenial
```

### rkt

1: Find out the container uuid:

```console
$ rkt run --interactive=true docker://busybox
$ rkt list
UUID            APP     IMAGE NAME                                      STATE   CREATED         STARTED         NETWORKS
c2d2e87e        busybox registry-1.docker.io/library/busybox:latest     running 6 minutes ago   6 minutes ago   default:ip4=172.16.28.3
```

2: Attach with cntr

```console
# make sure your container is still running!
$ cntr attach c2d2e87e
# Finally not the old ugly top!
[gen0@rkt-c2d2e87e-e798-4341-ae93-26f6cbb7c017:/var/lib/cntr]# htop
...
```

With cntr you can also debug stage1 of rkt - even there is no support from rkt itself.

```console
$ ps aux | grep stage1
joerg    13546  0.0  0.0 120808  1608 pts/12   S+   11:10   0:00 grep --binary-files=without-match --directories=skip --color=auto stage1
root     22232  0.0  0.0  54208  2656 pts/7    S+   10:54   0:00 stage1/rootfs/usr/lib/ld-linux-x86-64.so.2 stage1/rootfs/usr/bin/systemd-nspawn --boot --notify-ready=yes --register=true --link-journal=try-guest --quiet --uuid=c2d2e87e-e798-4341-ae93-26f6cbb7c017 --machine=rkt-c2d2e87e-e798-4341-ae93-26f6cbb7c017 --directory=stage1/rootfs --capability=CAP_AUDIT_WRITE,CAP_CHOWN,CAP_DAC_OVERRIDE,CAP_FSETID,CAP_FOWNER,CAP_KILL,CAP_MKNOD,CAP_NET_RAW,CAP_NET_BIND_SERVICE,CAP_SETUID,CAP_SETGID,CAP_SETPCAP,CAP_SETFCAP,CAP_SYS_CHROOT -- --default-standard-output=tty --log-target=null --show-status=0
```

Therefore we use the process id instead of the container uuid:

```console
$ cntr attach 22232
# new and exiting territory!
[root@turingmachine:/var/lib/cntr]# mount | grep pods
sysfs on /var/lib/cntr/var/lib/rkt/pods/run/c2d2e87e-e798-4341-ae93-26f6cbb7c017/stage1/rootfs/sys type sysfs (ro,nosuid,nodev,noexec,relatime)
tmpfs on /var/lib/cntr/var/lib/rkt/pods/run/c2d2e87e-e798-4341-ae93-26f6cbb7c017/stage1/rootfs/sys/fs/cgroup type tmpfs (ro,nosuid,nodev,noexec,mode=755)
cgroup on /var/lib/cntr/var/lib/rkt/pods/run/c2d2e87e-e798-4341-ae93-26f6cbb7c017/stage1/rootfs/sys/fs/cgroup/memory type cgroup (ro,nosuid,nodev,noexec,relatime,memory)
```

### systemd-nspawn

1: Start container

```console
$ wget https://cloud-images.ubuntu.com/releases/16.04/release/ubuntu-16.04-server-cloudimg-amd64-root.tar.xz
$ mkdir /var/lib/machines/ubuntu
$ tar -xf ubuntu-16.04-server-cloudimg-amd64-root.tar.xz -C /var/lib/machines/ubuntu
$ systemd-nspawn -b -M ubuntu
$ machinectl list
MACHINE CLASS     SERVICE        OS     VERSION ADDRESSES
ubuntu  container systemd-nspawn ubuntu 16.04   -
```

2: Attach
```console
$ cntr attach ubuntu
```

### Generic process id

The minimal information needed by cntr is the process id of a container process you want to attach to.

```console
# Did you now chromium uses namespaces too?
$ ps aux | grep 'chromium --type=renderer'
joerg    17498 11.7  1.0 1394504 174256 ?      Sl   15:16   0:08 /usr/bin/chromium
```

In this case 17498 is the pid we are looking for.

```console
$ cntr attach 17498
# looks quite similar to our system, but with less users
[joerg@turingmachine cntr]$ ls -la /
total 240
drwxr-xr-x   23 nobody nogroup    23 Mar 13 15:05 .
drwxr-xr-x   23 nobody nogroup    23 Mar 13 15:05 ..
drwxr-xr-x    2 nobody nogroup     3 Mar 13 15:14 bin
drwxr-xr-x    4 nobody nogroup 16384 Jan  1  1970 boot
drwxr-xr-x   24 nobody nogroup  4120 Mar 13 14:56 dev
drwxr-xr-x   52 nobody nogroup   125 Mar 13 15:14 etc
drwxr-xr-x    3 nobody nogroup     3 Jan  8 16:17 home
drwxr-xr-x    8 nobody nogroup     8 Feb  9 22:10 mnt
dr-xr-xr-x  306 nobody nogroup     0 Mar 13 09:38 proc
drwx------   22 nobody nogroup    43 Mar 13 15:09 root
...
```

### Containerd

For containerd integration the `ctr` binary is required. You can get a binary by running:

``` console
$ GOPATH=$(mktemp -d)
$ go get github.com/containerd/containerd/cmd/ctr
$ $GOPATH/bin/ctr --help
```

Put the resulting `ctr` binary in your `$PATH`

1: Start container
```console
$ ctr images pull docker.io/library/busybox:latest
$ ctr run docker.io/library/busybox:latest boxbusy
$ ctr tasks lists
TASK        PID      STATUS
boxbusy    24310    RUNNING
```

2: Attach
```console
$ cntr attach boxbusy
```

It's also possible to run cntr from a container itself.
This repository contains a example Dockerfile for that:

```console
$ docker build -f Dockerfile.example . -t cntr
$ docker save cntr > cntr.tar
$ ctr images import --base-name cntr ./cntr.tar
```

In this example we attach to containerd by process id. The proccess id of a task is given in `ctr tasks list`.

```console
$ ctr run --privileged --with-ns pid:/proc/1/ns/pid --tty docker.io/library/cntr:latest cntr /usr/bin/cntr attach 31523 /bin/sh
```

To resolve containerd names one also would need to add the `ctr` binary (~12mb) to the Dockerfile.

## Additional Config

### ZFS

`cntr` requires POSIX ACLs be enabled under ZFS. By default, Linux ZFS doesn't have POSIX ACLs enabled. This results in
the following error when trying to `attach`:

```console
unable to move container mounts to new mountpoint: EOPNOTSUPP: Operation not supported on transport endpoint
```

To enable POSIX ACLs on the ZFS dataset:

```console
$ zfs set acltype=posixacl zpool/media
$ zfs set xattr=sa zpool/media              #  optional, but encouraged for best perfomance
```

# How it works

Cntr is container-agnostic: Instead of interfacing with container engines, it
implements the underlying operating system API. It treats every container as a
group of processes, that it can inherit properties from.

Cntr inherits the following container properties:
  * Namespaces (mount, uts, pid, net, cgroup, ipc)
  * Cgroups
  * Apparamor/selinux
  * Capabilities
  * User/group ids
  * Environment variables
  * The following files: /etc/passwd, /etc/hostname, /etc/hosts, /etc/resolv.conf

Under the hood it spawns a shell or user defined program that inherits the full
context of the container and mount itself as a fuse filesystem.

We extensively evaluated the correctness and performance of cntr's filesystem
using [xfstests](https://github.com/Mic92/xfstests-cntr) and a wide range of
filesystem performance benchmarks (iozone, pgbench, dbench, fio, fs-mark,
postmark, ...)

# Related projects
- [nsenter](https://manpages.debian.org/testing/manpages-de/nsenter.1.de.html)
  - Only covers linux namespaces and the user is limited to tools installed in the
    containers
- [toolbox](https://github.com/coreos/toolbox)
  - Does attach from a container to the host, this is the opposite of what Cntr
    is doing

# Bibtex

We published a paper with all technical details about Cntr in
[Usenix ATC 2018](https://www.usenix.org/conference/atc18/presentation/thalheim).

```bibtex
@inproceedings{cntr-atc18,
  author = {J{\"o}rg Thalheim and Pramod Bhatotia and Pedro Fonseca and Baris Kasikci},
  title = {Cntr: Lightweight {OS} Containers},
  booktitle = {2018 {USENIX} Annual Technical Conference ({USENIX} {ATC} 18)},
  year = {2018},
}
```
