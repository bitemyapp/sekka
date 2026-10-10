{ pkgs }:
pkgs.rustPlatform.buildRustPackage {
  pname = "genkan-vm-fixture";
  version = "0.1.0";
  src = pkgs.lib.fileset.toSource {
    root = ../installer/genkan;
    fileset = pkgs.lib.fileset.unions [
      ../installer/genkan/rust/Cargo.toml
      ../installer/genkan/rust/src
      ../installer/genkan/tests/vm-fixture/Cargo.toml
      ../installer/genkan/tests/vm-fixture/Cargo.lock
      ../installer/genkan/tests/vm-fixture/src
    ];
  };
  cargoRoot = "tests/vm-fixture";
  buildAndTestSubdir = "tests/vm-fixture";
  cargoLock.lockFile = ../installer/genkan/tests/vm-fixture/Cargo.lock;
  # Test-only binary: neither the live ISO nor installed systems include it.
}
