{ pkgs ? import <nixpkgs> {}
, src ? ./.
, naersk-lib
}:
with pkgs;

naersk-lib.buildPackage rec {
  name = "cntr";
  inherit src;

  meta = with stdenv.lib; {
    description = "A container debugging tool based on FUSE";
    homepage = "https://github.com/Mic92/cntr";
    license = licenses.mit;
    maintainers = with maintainers; [ mic92 ];
    platforms = platforms.unix;
  };
 }
