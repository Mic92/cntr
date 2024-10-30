{
  description = "A container debugging tool based on FUSE";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
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
          packages.cntr = pkgs.callPackage ./. {
            src = self;
          };
          packages.default = config.packages.cntr;
          devShells.default = pkgs.mkShell {
            buildInputs = [
              pkgs.cargo
              pkgs.cargo-watch
              pkgs.rustc
              pkgs.clippy
              pkgs.cargo-bloat
              pkgs.rust-analyzer
            ];
          };
          checks =
            lib.optionalAttrs (!pkgs.hostPlatform.isRiscV64) {
              inherit
                (pkgs.callPackages ./vm-test.nix {
                  inherit (config.packages) cntr;
                })
                docker
                podman
                ;
            }
            // lib.mapAttrs' (n: lib.nameValuePair "package-${n}") config.packages;
        };
    };
}
