{ flake ? builtins.getFlake (toString ./.)
, pkgs ? flake.inputs.nixpkgs.legacyPackages.${builtins.currentSystem}
, makeTest ? pkgs.callPackage (flake.inputs.nixpkgs + "/nixos/tests/make-test-python.nix")
, cntr ? flake.defaultPackage.${builtins.currentSystem}
}:

let
  makeTest' = test: makeTest test {
    inherit pkgs;
    inherit (pkgs) system;
  };
  ociTest = { pkgs, ... }: {
    virtualisation.oci-containers.containers.nginx = {
      image = "nginx-container";
      imageFile = pkgs.dockerTools.examples.nginx;
      ports = ["8181:80"];
    };

    environment.systemPackages = [
      cntr
    ];
  };
in
{
  docker = makeTest' {
    name = "docker";
    nodes.server = { ... }: {
      imports = [ ociTest ];
      virtualisation.oci-containers.backend = "docker";
    };

    testScript = ''
      start_all()
      server.wait_for_unit("docker-nginx.service")
      server.wait_for_open_port(8181)
      server.succeed("cntr attach nginx true")
    '';
  };
  podman = makeTest' {
    name = "podman";
    nodes.server = { ... }: {
      imports = [ ociTest ];
      virtualisation.oci-containers.backend = "podman";
    };

    testScript = ''
      start_all()
      server.wait_for_unit("podman-nginx.service")
      server.wait_for_open_port(8181)
      server.succeed("cntr attach nginx true")
    '';
  };
}
