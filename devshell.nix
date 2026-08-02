{
  pkgs,
  fenix ? null,
}:
if pkgs.stdenv.isDarwin then
  let
    crossPkgs = pkgs.pkgsCross.musl64;
    crossToolchain = crossPkgs.stdenv.cc;
    rustTargetTriple = "x86_64-unknown-linux-musl";
    fenixPackages = fenix.packages.${pkgs.stdenv.hostPlatform.system};
    rustToolchain = fenixPackages.stable.withComponents [
      "cargo"
      "clippy"
      "rust-src"
      "rustc"
    ];
    rustToolchainWithTarget = fenixPackages.combine [
      rustToolchain
      fenixPackages.targets.${rustTargetTriple}.stable.rust-std
    ];
  in
  pkgs.mkShell {
    buildInputs = [
      rustToolchainWithTarget
      pkgs.rust-analyzer
      crossToolchain
    ];
    CARGO_BUILD_TARGET = rustTargetTriple;
    CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER = "${crossToolchain}/bin/${crossPkgs.stdenv.cc.targetPrefix}cc";
  }
else
  pkgs.mkShell {
    buildInputs = [
      pkgs.cargo
      pkgs.cargo-watch
      pkgs.rustc
      pkgs.clippy
      pkgs.rust-analyzer
    ];
    CNTR_TEST_SHELL = "${pkgs.pkgsStatic.busybox}/bin/sh";
  }
