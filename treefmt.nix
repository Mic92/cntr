{
  # Used to find the project root
  projectRootFile = "flake.lock";

  programs.nixfmt.enable = true;
  programs.rustfmt = {
    enable = true;
    edition = "2024";
  };
  programs.actionlint.enable = true;
}
