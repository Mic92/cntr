{
  testers,
  cntr,
  pkgs,
  lib,
}:

let
  # Busybox image for testing both attach and exec
  busyboxImage = pkgs.dockerTools.buildLayeredImage {
    name = "busybox-test";
    tag = "latest";
    contents = [ pkgs.busybox ];
    config.Cmd = [
      "${pkgs.busybox}/bin/sleep"
      "infinity"
    ];
  };

  ociTest = {
    virtualisation.oci-containers.containers.busybox = {
      image = "busybox-test:latest";
      imageFile = busyboxImage;
    };

    environment.systemPackages = [
      cntr
    ];
  };
in
{
  docker = testers.nixosTest {
    name = "docker";
    nodes.server = {
      imports = [ ociTest ];
      virtualisation.oci-containers.backend = "docker";
    };

    testScript = ''
      start_all()
      server.wait_for_unit("docker-busybox.service")
      server.succeed("cntr attach busybox true")
      server.succeed("cntr exec busybox -- /bin/sh -c 'echo exec test passed'")
    '';
  };
  podman = testers.nixosTest {
    name = "podman";
    nodes.server = {
      imports = [ ociTest ];
      virtualisation.oci-containers.backend = "podman";
    };

    testScript = ''
      start_all()
      server.wait_for_unit("podman-busybox.service")
      server.succeed("cntr attach busybox true")
      server.succeed("cntr exec busybox -- /bin/sh -c 'echo exec test passed'")
    '';
  };

  nspawn = testers.nixosTest {
    name = "nspawn";
    nodes.server =
      { pkgs, ... }:
      let
        # Minimal init script for the container
        initScript = pkgs.writeScript "container-init" ''
          #!${pkgs.pkgsStatic.busybox}/bin/sh
          exec ${pkgs.pkgsStatic.busybox}/bin/sleep infinity
        '';
      in
      {
        environment.systemPackages = [
          cntr
          pkgs.strace
        ];

        # Enable systemd-nspawn
        systemd.targets.machines.wants = [ "systemd-nspawn@testcontainer.service" ];

        # Configure nspawn to not boot, just run our init
        systemd.nspawn.testcontainer = {
          execConfig = {
            Boot = false;
            Parameters = initScript;
          };
          filesConfig = {
            BindReadOnly = "/nix/store";
          };
        };

        # Create a minimal container root with static busybox
        systemd.tmpfiles.rules = [
          "d /var/lib/machines/testcontainer 0755 root root - -"
          "d /var/lib/machines/testcontainer/bin 0755 root root - -"
          "d /var/lib/machines/testcontainer/tmp 0755 root root - -"
          "d /var/lib/machines/testcontainer/etc 0755 root root - -"
          "L+ /var/lib/machines/testcontainer/bin/sh - - - - ${pkgs.pkgsStatic.busybox}/bin/sh"
          "L+ /var/lib/machines/testcontainer/bin/echo - - - - ${pkgs.pkgsStatic.busybox}/bin/echo"
          "f /var/lib/machines/testcontainer/etc/os-release 0644 root root - NAME=test"
        ];
      };

    testScript = ''
      start_all()
      server.wait_for_unit("systemd-nspawn@testcontainer.service")
      server.succeed("machinectl status testcontainer")
      # Test attach and exec
      server.succeed("cntr attach -t nspawn testcontainer true")
      server.succeed("cntr exec -t nspawn testcontainer -- /bin/echo 'exec test passed'")
    '';
  };

  builder =
    let
      blockingDrv = pkgs.writeText "blocking-build.nix" ''
        let
          bb = builtins.storePath "${pkgs.pkgsStatic.busybox}";
        in
        derivation {
          name = "cntr-test-blocking-build";
          system = "${pkgs.stdenv.hostPlatform.system}";
          builder = bb + "/bin/sh";
          args = [ "-c" "''${bb}/bin/sleep 99999" ];
        }
      '';
    in
    testers.nixosTest {
      name = "builder";
      nodes.server =
        { pkgs, ... }:
        {
          environment.systemPackages = [
            cntr
            pkgs.pkgsStatic.busybox
          ];
          # Disable substituters to avoid network timeouts in the VM
          nix.settings.substituters = pkgs.lib.mkForce [ ];

          systemd.services.blocking-build = {
            description = "Blocking Nix build for testing";
            serviceConfig = {
              Type = "simple";
              ExecStart = "${pkgs.nix}/bin/nix-build ${blockingDrv}";
            };
          };
        };

      testScript = ''
        start_all()

        # Start the blocking build service
        server.succeed("systemctl start blocking-build")

        # Wait for the sleep process to start
        server.wait_until_succeeds("pgrep sleep", timeout=30)

        # Test attaching to the builder using the command backend
        # The command backend searches for the pattern in /proc/*/cmdline
        server.succeed("cntr attach -t command 'sleep 99999' true")
        server.succeed("cntr exec -t command 'sleep 99999' -- /bin/sh -c 'echo exec test passed'")

        # Clean up
        server.succeed("pkill sleep")
      '';
    };

  apparmor = testers.nixosTest {
    name = "apparmor";
    nodes.server =
      { pkgs, lib, ... }:
      {
        imports = [ ociTest ];
        virtualisation.oci-containers.backend = "docker";

        environment.systemPackages = [
          pkgs.apparmor-bin-utils
          pkgs.jq
        ];

        # Enable AppArmor
        security.apparmor = {
          enable = true;

          # Custom AppArmor profile for Docker containers
          policies.docker-default = {
            state = "enforce";
            profile = ''
              abi <abi/4.0>,
              include <tunables/global>

              profile docker-default flags=(attach_disconnected,mediate_deleted) {
                include <abstractions/base>

                network,
                capability,
                file,
                umount,

                # Deny access to /tmp/restricted specifically
                deny /tmp/restricted/** rwklx,

                # Allow everything else in /tmp
                /tmp/** rw,

                # Allow nix store access
                /nix/store/** mr,

                # Allow proc/sys access
                /proc/** r,
                /sys/** r,
              }
            '';
          };
        };

        # Add a second container with AppArmor profile applied
        virtualisation.oci-containers.containers.busybox-apparmor = {
          image = "busybox-test:latest";
          imageFile = busyboxImage;
          cmd = [
            "${pkgs.busybox}/bin/sleep"
            "infinity"
          ];
          extraOptions = [
            "--security-opt"
            "apparmor=docker-default"
          ];
        };
      };

    testScript = ''
      start_all()

      with subtest("AppArmor is enabled and profile is loaded"):
          server.wait_for_unit("apparmor.service")
          server.succeed("systemctl status apparmor.service")
          server.succeed("aa-status --json | jq -e '.profiles.\"docker-default\" == \"enforce\"'")

      with subtest("Regular container without AppArmor works"):
          server.wait_for_unit("docker-busybox.service")
          server.succeed("cntr attach busybox true")
          server.succeed("cntr exec busybox -- /bin/sh -c 'echo exec test passed'")

      with subtest("Container with AppArmor profile works with cntr"):
          server.wait_for_unit("docker-busybox-apparmor.service")

          # Verify AppArmor profile is applied to the container
          apparmor_profile = server.succeed("docker inspect -f '{{.AppArmorProfile}}' busybox-apparmor").strip()
          print(f"AppArmor profile on container: {apparmor_profile}")
          assert apparmor_profile == "docker-default", f"Expected docker-default, got {apparmor_profile}"

          # cntr should work with AppArmor-confined container
          server.succeed("cntr attach busybox-apparmor true")
          server.succeed("cntr exec busybox-apparmor -- /bin/sh -c 'echo exec test passed'")
    '';
  };

  k3s =
    let
      imageEnv = pkgs.buildEnv {
        name = "cntr-test-image-env";
        paths = [
          pkgs.tini
          (lib.hiPrio pkgs.coreutils)
          pkgs.busybox
        ];
      };
      # Pause image for k3s (similar to nixpkgs k3s tests)
      pauseImage = pkgs.dockerTools.streamLayeredImage {
        name = "test.local/pause";
        tag = "local";
        contents = imageEnv;
        config.Entrypoint = [
          "/bin/tini"
          "--"
          "/bin/sleep"
          "inf"
        ];
      };
      # Test container image
      testImage = pkgs.dockerTools.streamLayeredImage {
        name = "test.local/cntr-test";
        tag = "local";
        contents = imageEnv;
        config.Entrypoint = [
          "/bin/sleep"
          "infinity"
        ];
      };
      testPodYaml = pkgs.writeText "test-pod.yml" ''
        apiVersion: v1
        kind: Pod
        metadata:
          name: cntr-test
        spec:
          containers:
          - name: test-container
            image: test.local/cntr-test:local
            imagePullPolicy: Never
            command: ["sleep", "infinity"]
      '';
    in
    testers.nixosTest {
      name = "k3s";
      nodes.server =
        { pkgs, ... }:
        {
          environment.systemPackages = [
            cntr
            pkgs.k3s
          ];

          # k3s uses enough resources the default vm fails.
          virtualisation.memorySize = 1536;
          virtualisation.diskSize = 4096;

          services.k3s.enable = true;
          services.k3s.role = "server";
          # Reduce resource usage by disabling unnecessary components
          services.k3s.extraFlags = [
            "--disable coredns"
            "--disable local-storage"
            "--disable metrics-server"
            "--disable servicelb"
            "--disable traefik"
            "--pause-image test.local/pause:local"
          ];
        };

      testScript = ''
        start_all()
        server.wait_for_unit("k3s")
        server.succeed("kubectl cluster-info")

        # Import the pause image first (required for pod sandboxes)
        server.succeed("${pauseImage} | ctr --namespace k8s.io image import -")

        # Import the test image
        server.succeed("${testImage} | ctr --namespace k8s.io image import -")

        # Wait for service account to be ready
        server.wait_until_succeeds("kubectl get serviceaccount default")

        # Create and wait for the test pod
        server.succeed("kubectl apply -f ${testPodYaml}")
        server.succeed("kubectl wait --timeout=60s --for=condition=Ready pod/cntr-test")

        # Get the container ID from k3s/containerd
        container_id = server.succeed(
            "crictl ps --name test-container -q"
        ).strip()

        # Test cntr attach and exec with the container
        server.succeed(f"cntr attach {container_id} true")
        server.succeed(f"cntr exec {container_id} -- /bin/sh -c 'echo exec test passed'")
      '';
    };
}
