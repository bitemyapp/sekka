{ pkgs }:
pkgs.rustPlatform.buildRustPackage {
  pname = "genkan-vm-fixture";
  version = "0.1.0";
  # Sekka's paths, as the path dependencies name them: the backend and the
  # typestate crate it uses.
  src = pkgs.lib.fileset.toSource {
    root = ../.;
    fileset = pkgs.lib.fileset.unions [
      ../installer/genkan/rust/Cargo.toml
      ../installer/genkan/rust/src
      ../installer/genkan/tests/vm-fixture/Cargo.toml
      ../installer/genkan/tests/vm-fixture/Cargo.lock
      ../installer/genkan/tests/vm-fixture/src
      ../desktop/aizome/crates/aizome-typestate
    ];
  };
  cargoRoot = "installer/genkan/tests/vm-fixture";
  buildAndTestSubdir = "installer/genkan/tests/vm-fixture";
  cargoLock.lockFile = ../installer/genkan/tests/vm-fixture/Cargo.lock;
  # Test-only binary: neither the live ISO nor installed systems include it.
}
