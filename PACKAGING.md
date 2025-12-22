# Packaging Notes

This document provides guidance for distribution packagers.

## Build Requirements

- Rust 2024 edition (rustc 1.85+)
- cargo
- Linux kernel headers (for libc bindings)

## Runtime Requirements

- Linux kernel 5.2 or later (uses `fsopen`/`fsmount` mount API)
- Root privileges or appropriate file capabilities (see [setcap section in README](README.md#running-with-file-capabilities-setcap))

## Build Instructions

```sh
cargo build --release --locked
install -Dm755 target/release/cntr /usr/bin/cntr
```

## Shell Completions

Pre-built shell completions are provided in the `completions/` directory:

| Shell | Source | Install Location |
|-------|--------|------------------|
| Bash | `completions/cntr.bash` | `/usr/share/bash-completion/completions/cntr` |
| Zsh | `completions/cntr.zsh` | `/usr/share/zsh/site-functions/_cntr` |
| Fish | `completions/cntr.fish` | `/usr/share/fish/vendor_completions.d/cntr.fish` |
| Nushell | `completions/cntr.nu` | `/usr/share/nushell/completions/cntr.nu` |

The packaging files in `contrib/` already include these installations.

For Nushell, users can also source the completion directly:
```nushell
use /usr/share/nushell/completions/cntr.nu *
```

## Manpage

The manpage source is at `doc/cntr.1.scd` in scdoc format. Build with:

```sh
scdoc < doc/cntr.1.scd > cntr.1
install -Dm644 cntr.1 /usr/share/man/man1/cntr.1
```

The packaging files in `contrib/` already include manpage generation.

## Dependencies

cntr is statically linked by default and has no runtime library dependencies.

The Rust crate dependencies are:
- `libc` - Linux syscall bindings
- `nix` - Higher-level Unix API
- `container-pid` - Container PID resolution
- `anyhow` - Error handling
- `log` - Logging facade

## Distribution Packaging Files

Ready-to-use packaging files are provided in the `contrib/` directory:

| Distribution | Path | Notes |
|--------------|------|-------|
| Arch Linux | [`contrib/arch/PKGBUILD`](contrib/arch/PKGBUILD) | AUR-compatible |
| Fedora/RHEL | [`contrib/fedora/cntr.spec`](contrib/fedora/cntr.spec) | RPM spec file |
| Debian/Ubuntu | [`contrib/debian/`](contrib/debian/) | debhelper packaging |
| Alpine Linux | [`contrib/alpine/APKBUILD`](contrib/alpine/APKBUILD) | aports-compatible |
| Gentoo | [`contrib/gentoo/`](contrib/gentoo/app-containers/cntr/) | ebuild (needs CRATES variable) |
| Nix/NixOS | [`flake.nix`](flake.nix), [`default.nix`](default.nix) | Available in nixpkgs |

### Nix / NixOS

cntr is available in nixpkgs:

```sh
nix-shell -p cntr
# or
nix profile install nixpkgs#cntr
```

The flake in this repository can also be used directly:

```sh
nix run github:Mic92/cntr -- attach <container>
```

### Gentoo Note

The ebuild requires populating the `CRATES` variable. Use `cargo-ebuild` to generate the full ebuild with dependencies:

```sh
cargo ebuild
```

## Testing

The repository includes NixOS VM tests that verify functionality across container runtimes:

```sh
nix build .#checks.x86_64-linux.vm-test
```

For manual testing:

```sh
# Test with docker
docker run -d --name test alpine sleep infinity
cntr attach test
cntr exec test -- cat /etc/os-release

# Test with process ID
cntr attach <pid>
```

## Upstream Contact

- Repository: https://github.com/Mic92/cntr
- Author: Jörg Thalheim <joerg@thalheim.io>
