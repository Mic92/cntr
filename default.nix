{
  rustPlatform,
  lib,
  clippy,
  self,
  pkgsStatic,
  scdoc,
  installShellFiles,
  withClippy ? false,
}:

let
  package = rustPlatform.buildRustPackage {
    name = "cntr";
    src = lib.sources.sourceFilesBySuffices self [
      ".rs"
      ".toml"
      ".lock"
      ".scd"
      ".bash"
      ".zsh"
      ".fish"
      ".nu"
    ];
    cargoLock.lockFile = ./Cargo.lock;

    nativeBuildInputs = [
      clippy
      scdoc
      installShellFiles
    ];

    # Provide statically-linked shell for integration tests
    CNTR_TEST_SHELL = "${pkgsStatic.busybox}/bin/sh";

    postInstall = ''
      # Build and install manpage
      scdoc < doc/cntr.1.scd > cntr.1
      installManPage cntr.1

      # Install shell completions
      installShellCompletion --cmd cntr \
        --bash completions/cntr.bash \
        --zsh completions/cntr.zsh \
        --fish completions/cntr.fish \
        --nushell completions/cntr.nu
    '';

    meta = with lib; {
      description = "A container debugging tool based on Linux mount API";
      homepage = "https://github.com/Mic92/cntr";
      license = licenses.mit;
      maintainers = with maintainers; [ mic92 ];
      platforms = platforms.unix;
    };
  };
in
if withClippy then
  package.overrideAttrs (oldAttrs: {
    buildPhase = ''
      cargo clippy --release --locked -- -D warnings
    '';
    installPhase = ''
      touch $out
    '';
  })
else
  package
