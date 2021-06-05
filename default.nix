{ pkgs ? import <nixpkgs> {}
, src ? ./.
}:
with pkgs;

pkgs.rustPlatform.buildRustPackage {
  name = "cntr";
  inherit src;
  cargoLock.lockFile = ./Cargo.lock;
  meta = with stdenv.lib; {
    description = "A container debugging tool based on FUSE";
    homepage = "https://github.com/Mic92/cntr";
    license = licenses.mit;
    maintainers = with maintainers; [ mic92 ];
    platforms = platforms.unix;
  };
}
