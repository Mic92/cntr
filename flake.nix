{
  description = "A container debugging tool based on FUSE";

  inputs = {
    nixpkgs.url = "git+https://github.com/NixOS/nixpkgs?shallow=1&ref=nixpkgs-unstable";
    flake-parts.url = "github:hercules-ci/flake-parts";
    flake-parts.inputs.nixpkgs-lib.follows = "nixpkgs";

    treefmt-nix.url = "github:numtide/treefmt-nix";
    treefmt-nix.inputs.nixpkgs.follows = "nixpkgs";

    fenix.url = "github:nix-community/fenix";
    fenix.inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs =
    inputs@{ flake-parts, self, ... }:
    flake-parts.lib.mkFlake { inherit inputs; } {
      imports = [ ./treefmt.nix ];
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "riscv64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];
      perSystem =
        {
          pkgs,
          config,
          lib,
          ...
        }:
        {
          packages = lib.optionalAttrs (!pkgs.stdenv.isDarwin) {
            cntr = pkgs.callPackage ./default.nix {
              inherit self;
              inherit (pkgs) pkgsStatic;
            };
            default = config.packages.cntr;
          };
          devShells.default =
            if pkgs.stdenv.isDarwin then
              let
                crossPkgs = pkgs.pkgsCross.musl64;
                crossToolchain = crossPkgs.stdenv.cc;
                rustTargetTriple = "x86_64-unknown-linux-musl";
                rustToolchain = inputs.fenix.packages.${pkgs.system}.stable.withComponents [
                  "cargo"
                  "clippy"
                  "rust-src"
                  "rustc"
                ];
                rustToolchainWithTarget = inputs.fenix.packages.${pkgs.system}.combine [
                  rustToolchain
                  inputs.fenix.packages.${pkgs.system}.targets.${rustTargetTriple}.stable.rust-std
                ];
              in
              pkgs.mkShell {
                buildInputs = [
                  rustToolchainWithTarget
                  pkgs.rust-analyzer
                  crossToolchain
                ];
                CARGO_BUILD_TARGET = rustTargetTriple;
                CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER = "${crossToolchain}/bin/${crossPkgs.stdenv.cc.targetPrefix}cc";
              }
            else
              pkgs.mkShell {
                buildInputs = [
                  pkgs.cargo
                  pkgs.cargo-watch
                  pkgs.rustc
                  pkgs.clippy
                  pkgs.rust-analyzer
                ];
                CNTR_TEST_SHELL = "${pkgs.pkgsStatic.busybox}/bin/sh";
              };
          checks = lib.optionalAttrs (!pkgs.stdenv.isDarwin) (
            {
              clippy = (
                config.packages.cntr.override {
                  withClippy = true;
                }
              );
              shell-completions = pkgs.callPackage ./shell-completion-test.nix {
                inherit (config.packages) cntr;
              };
            }
            // lib.optionalAttrs (!pkgs.stdenv.hostPlatform.isRiscV64) {
              inherit
                (pkgs.callPackages ./vm-test.nix {
                  inherit (config.packages) cntr;
                })
                docker
                podman
                podman-rootless
                nspawn
                k3s
                builder
                apparmor
                ;
            }
            // lib.mapAttrs' (n: lib.nameValuePair "package-${n}") config.packages
          );
        };
    };
}
