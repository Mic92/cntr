{
  description = "A container debugging tool based on FUSE";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, utils }:
    (utils.lib.eachDefaultSystem (system: let
      pkgs = import nixpkgs { inherit system; };
    in {
      packages.cntr = pkgs.callPackage ./. {
        src = self;
      };
      defaultPackage = self.packages.${system}.cntr;
      devShell = pkgs.mkShell {
        buildInputs = [
          pkgs.cargo
          pkgs.cargo-watch
          pkgs.rustc
          pkgs.clippy
          pkgs.cargo-bloat
          pkgs.rust-analyzer
        ];
        #buildInputs = [ pkgs.pkgsMusl.cargo pkgs.pkgsMusl.rustc ];
      };
    })) // {
    checks.x86_64-linux = let
      system = "x86_64-linux";
      pkgs = nixpkgs.legacyPackages.${system};
    in {
      inherit (import ./vm-test.nix {
        makeTest = import (nixpkgs + "/nixos/tests/make-test-python.nix");
        inherit pkgs;
        inherit (self.packages.${system}) cntr;
      }) docker podman;
    };
  };
}
