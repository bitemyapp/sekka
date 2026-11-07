# SPDX-License-Identifier: GPL-3.0-or-later
# Sekka's first-boot setup helper (installer/genkan/first-boot): its own
# Cargo workspace and lock, built with Genkan's backend (rust/), the Mac
# installer's contract crate (macos/crates/contract) and Yukimi's
# configuration editing (apps/yukimi/crates/yukimi-config), which it depends
# on by path. Only those sources are copied, at the paths the manifests
# name, so neither Genkan's window, Yukimi nor Aizome rebuilds it, nor it
# them.
{
  lib,
  rustPlatform,
  runCommand,
  tzdata,
}:
let
  helper = lib.fileset.toSource {
    root = ./.;
    fileset = lib.fileset.unions [
      ./Cargo.toml
      ./Cargo.lock
      ./offered.json
      ./src
      # setup.nix as the helper writes it, which a test holds it to.
      ./test-data
    ];
  };
  backend = lib.fileset.toSource {
    root = ../rust;
    fileset = lib.fileset.unions [
      ../rust/Cargo.toml
      ../rust/src
    ];
  };
  contract = lib.fileset.toSource {
    root = ../macos;
    fileset = lib.fileset.unions [
      ../macos/Cargo.toml
      ../macos/crates/contract/Cargo.toml
      ../macos/crates/contract/src
      ../macos/crates/contract/profiles
    ];
  };
  # The workspace manifest only for what yukimi-config inherits from it.
  yukimi = lib.fileset.toSource {
    root = ../../../apps/yukimi;
    fileset = lib.fileset.unions [
      ../../../apps/yukimi/Cargo.toml
      ../../../apps/yukimi/crates/yukimi-config/Cargo.toml
      ../../../apps/yukimi/crates/yukimi-config/src
    ];
  };
  source = runCommand "sekka-setup-source" { } ''
    mkdir -p $out/installer/genkan $out/apps
    cp -r ${helper} $out/installer/genkan/first-boot
    cp -r ${backend} $out/installer/genkan/rust
    cp -r ${contract} $out/installer/genkan/macos
    cp -r ${yukimi} $out/apps/yukimi
  '';
in
rustPlatform.buildRustPackage {
  pname = "sekka-setup-helper";
  version = "0.1.0";
  src = source;
  sourceRoot = "sekka-setup-source/installer/genkan/first-boot";
  cargoLock.lockFile = ./Cargo.lock;
  # The transaction, protocol, handoff and Genkan-validation tests, with a
  # zone database for the time zone checks.
  doCheck = true;
  preCheck = ''
    export TZDIR=${tzdata}/share/zoneinfo
  '';
  meta = {
    description = "Sekka's first-boot setup helper: one validated account and configuration, for the login screen";
    license = lib.licenses.gpl3Plus;
    mainProgram = "sekka-setup-helper";
  };
}
