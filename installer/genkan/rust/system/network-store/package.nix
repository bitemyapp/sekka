# SPDX-License-Identifier: GPL-3.0-or-later
{ lib, rustPlatform }:
rustPlatform.buildRustPackage {
  pname = "sekka-network-store";
  version = "0.1.0";
  # Only what the build reads.
  src = lib.fileset.toSource {
    root = ./.;
    fileset = lib.fileset.unions [
      ./Cargo.toml
      ./Cargo.lock
      ./src
    ];
  };
  cargoLock.lockFile = ./Cargo.lock;
  meta = {
    description = "Keeps NetworkManager's saved connections in a store sealed to the TPM";
    license = lib.licenses.gpl3Plus;
    mainProgram = "sekka-network-store";
  };
}
