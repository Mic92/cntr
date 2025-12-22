{
  description = "A container debugging tool based on FUSE";

  inputs = {
    nixpkgs.url = "git+https://github.com/NixOS/nixpkgs?shallow=1&ref=nixpkgs-unstable";
    flake-parts.url = "github:hercules-ci/flake-parts";
    flake-parts.inputs.nixpkgs-lib.follows = "nixpkgs";

    treefmt-nix.url = "github:numtide/treefmt-nix";
    treefmt-nix.inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs =
    inputs@{ flake-parts, self, ... }:
    flake-parts.lib.mkFlake { inherit inputs; } {
      imports = [ ./treefmt.nix ];
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "riscv64-linux"
      ];
      perSystem =
        {
          pkgs,
          config,
          lib,
          ...
        }:
        {
          packages.cntr = pkgs.callPackage ./default.nix {
            inherit self;
            inherit (pkgs) pkgsStatic;
          };
          packages.default = config.packages.cntr;
          devShells.default = pkgs.mkShell {
            buildInputs = [
              pkgs.cargo
              pkgs.cargo-watch
              pkgs.rustc
              pkgs.clippy
              pkgs.rust-analyzer
            ];
            CNTR_TEST_SHELL = "${pkgs.pkgsStatic.busybox}/bin/sh";
          };
          checks = {
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
              nspawn
              k3s
              builder
              apparmor
              ;
          }
          // lib.mapAttrs' (n: lib.nameValuePair "package-${n}") config.packages;
        };
    };
}
