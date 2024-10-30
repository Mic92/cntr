{
  rustPlatform,
  lib,
  clippy,
  self,
  withClippy ? false,
}:

let
  package = rustPlatform.buildRustPackage {
    name = "cntr";
    src = self;
    cargoLock.lockFile = ./Cargo.lock;

    nativeBuildInputs = [ clippy ];
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
