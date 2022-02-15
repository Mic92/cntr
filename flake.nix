{
  description = "A container debugging tool based on FUSE";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    utils.url = "github:numtide/flake-utils";
    flake-compat.url = "github:edolstra/flake-compat";
    flake-compat.flake = false;
    nix-filter.url = "github:numtide/nix-filter";
  };

  outputs =
    { self
    , nixpkgs
    , utils
    , flake-compat
    , nix-filter
    }
    @ inputs:
      (
        utils.lib.eachDefaultSystem (
          system: let
            pkgs = inputs.nixpkgs.legacyPackages.${system};
          in
            {
              packages.cntr = pkgs.callPackage ./default.lock.nix { inherit inputs; };
              defaultPackage = self.packages.${system}.cntr;
              devShell = nixpkgs.mkShell {
                buildInputs = [
                  pkgs.cargo
                  pkgs.rustc
                  pkgs.clippy
                  pkgs.cargo-bloat
                  pkgs.rust-analyzer
                ];
                #buildInputs = [ pkgs.pkgsMusl.cargo pkgs.pkgsMusl.rustc ];
              };
            }
        )
      )
      // {
        checks.x86_64-linux = let
          system = "x86_64-linux";
          pkgs = nixpkgs.legacyPackages.${system};
        in
          {
            integration-tests = import ./vm-test.nix {
              makeTest = import (nixpkgs + "/nixos/tests/make-test-python.nix");
              inherit pkgs;
              inherit (self.packages.${system}) cntr;
            };
          };
      };
}
