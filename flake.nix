{
  description = "A container debugging tool based on FUSE";

  inputs = {
    nixpkgs.url = "git+https://github.com/NixOS/nixpkgs?shallow=1&ref=nixpkgs-unstable";

    treefmt-nix.url = "github:numtide/treefmt-nix";
    treefmt-nix.inputs.nixpkgs.follows = "nixpkgs";

    fenix.url = "github:nix-community/fenix";
    fenix.inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs =
    {
      self,
      nixpkgs,
      treefmt-nix,
      fenix,
      ...
    }:
    let
      inherit (nixpkgs) lib;
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "riscv64-linux"
        "aarch64-darwin"
      ];
      forAllSystems = f: lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
      treefmtEval = forAllSystems (pkgs: treefmt-nix.lib.evalModule pkgs ./treefmt.nix);
    in
    {
      packages = forAllSystems (
        pkgs:
        lib.optionalAttrs (!pkgs.stdenv.isDarwin) rec {
          cntr = pkgs.callPackage ./default.nix {
            inherit self;
            inherit (pkgs) pkgsStatic;
          };
          default = cntr;
        }
      );

      formatter = forAllSystems (
        pkgs: treefmtEval.${pkgs.stdenv.hostPlatform.system}.config.build.wrapper
      );

      devShells = forAllSystems (pkgs: {
        default = pkgs.callPackage ./devshell.nix { inherit fenix; };
      });

      checks = forAllSystems (
        pkgs:
        pkgs.callPackages ./checks.nix {
          packages = self.packages.${pkgs.stdenv.hostPlatform.system};
          formatting = treefmtEval.${pkgs.stdenv.hostPlatform.system}.config.build.check self;
        }
      );
    };
}
