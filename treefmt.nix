{ inputs, ... }:
{
  imports = [ inputs.treefmt-nix.flakeModule ];

  perSystem =
    { pkgs, ... }:
    {
      treefmt = {
        # Used to find the project root
        projectRootFile = "flake.lock";

        programs.nixfmt.enable = (builtins.tryEval pkgs.nixfmt-rfc-style).success;
        programs.rustfmt.enable = true;
      };
    };
}
