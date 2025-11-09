{ inputs, ... }:
{
  imports = [ inputs.treefmt-nix.flakeModule ];

  perSystem =
    { pkgs, ... }:
    {
      treefmt = {
        # Used to find the project root
        projectRootFile = "flake.lock";
        flakeCheck = !pkgs.stdenv.hostPlatform.isRiscV64;

        programs.nixfmt.enable = true;
        programs.rustfmt = {
          enable = true;
          edition = "2024";
        };
        programs.actionlint.enable = true;
      };
    };
}
