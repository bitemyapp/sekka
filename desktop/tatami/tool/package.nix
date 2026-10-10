# SPDX-License-Identifier: MIT OR Apache-2.0
{ lib, rustPlatform }:
let
  # Content-addressed source: the installation media and installed systems
  # must produce the same derivation wherever this directory lives.
  source = builtins.path {
    path = ./.;
    name = "tatami-src";
    filter =
      path: _:
      !(builtins.elem (baseNameOf path) [
        "target"
        "package.nix"
      ]);
  };
in
rustPlatform.buildRustPackage {
  pname = "tatami";
  version = "0.1.0";
  src = source;
  cargoLock.lockFile = "${source}/Cargo.lock";
  meta = {
    description = "Session launcher and desktop commands for Tatami";
    license = with lib.licenses; [
      mit
      asl20
    ];
    mainProgram = "tatami";
  };
}
