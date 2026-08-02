{
  lib,
  pkgs,
  packages,
  formatting,
}:
lib.optionalAttrs (!pkgs.stdenv.hostPlatform.isRiscV64) {
  inherit formatting;
}
// lib.optionalAttrs (!pkgs.stdenv.isDarwin) (
  {
    clippy = packages.cntr.override { withClippy = true; };
    shell-completions = pkgs.callPackage ./shell-completion-test.nix {
      inherit (packages) cntr;
    };
  }
  // lib.optionalAttrs (!pkgs.stdenv.hostPlatform.isRiscV64) {
    inherit
      (pkgs.callPackages ./vm-test.nix {
        inherit (packages) cntr;
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
  // lib.mapAttrs' (n: lib.nameValuePair "package-${n}") packages
)
