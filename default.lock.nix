{ rustPlatform
, lib
, inputs
}:
rustPlatform.buildRustPackage rec {
  name = "cntr";
  src = inputs.nix-filter.lib.filter {
    root = ./.;
    exclude = [
      (inputs.nix-filter.lib.matchExt "nix")
      ./flake.lock
    ];
  };
  version =
    let
      commit = inputs.self.shortRev or "dirty";
      date = inputs.self.lastModifiedDate or inputs.self.lastModified or "19700101";
    in
      "${builtins.substring 0 8 date}_${commit}";

  cargoLock.lockFile = ./Cargo.lock;

  meta = with lib; {
    description = "A container debugging tool based on FUSE";
    homepage = "https://github.com/Mic92/cntr";
    license = licenses.mit;
    maintainers = with maintainers; [ mic92 ];
    platforms = platforms.unix;
  };
}
