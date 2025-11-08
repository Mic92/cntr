{
  rustPlatform,
  lib,
  clippy,
  self,
  pkgsStatic,
  withClippy ? false,
}:

let
  package = rustPlatform.buildRustPackage {
    name = "cntr";
    src = lib.sources.sourceFilesBySuffices self [
      ".rs"
      ".toml"
      ".lock"
    ];
    cargoLock.lockFile = ./Cargo.lock;

    nativeBuildInputs = [ clippy ];

    # Provide statically-linked shell for integration tests
    CNTR_TEST_SHELL = "${pkgsStatic.busybox}/bin/sh";

    meta = with lib; {
      description = "A container debugging tool based on FUSE";
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
