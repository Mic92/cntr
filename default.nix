{ pkgs ? import <nixpkgs> {}
, naersk-lib
}:
with pkgs;

let
   allowSource = { allow, src }:
    let
      out = builtins.filterSource filter src;
      filter = path: _fileType:
        lib.any (checkElem path) allow;
      checkElem = path: elem:
        lib.hasPrefix (toString elem) (toString path);
    in
    out;
in naersk-lib.buildPackage rec {
  name = "cntr";
  src = ./.;

  meta = with stdenv.lib; {
    description = "A container debugging tool based on FUSE";
    homepage = "https://github.com/Mic92/cntr";
    license = licenses.mit;
    maintainers = with maintainers; [ mic92 ];
    platforms = platforms.unix;
  };
 }
