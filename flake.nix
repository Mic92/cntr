{
  description = "A container debugging tool based on FUSE";

  inputs = {
    naersk.url = "github:nmattia/naersk/master";
    naersk.inputs.nixpkgs.follows = "nixpkgs";
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, naersk, utils }:
    (utils.lib.eachDefaultSystem (system: let
      pkgs = import nixpkgs { inherit system; };
      naersk-lib = pkgs.callPackage naersk { };
    in {
      packages.cntr = pkgs.callPackage ./. {
        inherit naersk-lib;
      };
      defaultPackage = self.packages.${system}.cntr;
      devShell = pkgs.mkShell {
        buildInputs = [ pkgs.cargo pkgs.rustc ];
      };
    })) // {
    checks.x86_64-linux = let
      system = "x86_64-linux";
      pkgs = nixpkgs.legacyPackages.${system};
    in {
      integration-tests = import ./vm-test.nix {
        makeTest = import (nixpkgs + "/nixos/tests/make-test-python.nix");
        inherit pkgs;
        inherit (self.packages.${system}) cntr;
      };
    };
  };
}
