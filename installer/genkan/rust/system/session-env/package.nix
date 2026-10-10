# SPDX-License-Identifier: GPL-3.0-or-later
{ lib, rustPlatform }:
let
  # Content-addressed source: the installation media and installed systems
  # must produce the same derivation wherever this directory lives.
  source = builtins.path {
    path = ./.;
    name = "calamares-session-env-src";
    filter =
      path: _:
      !(builtins.elem (baseNameOf path) [
        "target"
        "package.nix"
      ]);
  };
in
rustPlatform.buildRustPackage {
  pname = "calamares-session-env";
  version = "0.1.0";
  src = source;
  cargoLock.lockFile = "${source}/Cargo.lock";
  meta = {
    description = "Keeps one desktop session's environment out of the next";
    license = lib.licenses.gpl3Plus;
    mainProgram = "calamares-session-env";
  };
}
