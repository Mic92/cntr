with import <nixpkgs> {};
mkShell {
  nativeBuildInputs = [
    bashInteractive
    cargo
    rustc
  ];
}
