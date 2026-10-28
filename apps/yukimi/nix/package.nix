# SPDX-License-Identifier: MIT OR Apache-2.0
{
  lib,
  rustPlatform,
  pkg-config,
  wrapGAppsHook4,
  gtk4,
  libadwaita,
  sqlite,
}:
let
  # Content-addressed source: the same derivation wherever the checkout is.
  source = lib.fileset.toSource {
    root = ../../..;
    fileset = lib.fileset.unions [
      ../Cargo.toml
      ../Cargo.lock
      ../crates
      ../yukimi
      ../yukimi-helper
      # The completion checker uses Aizome's UI-independent IPC protocol.
      # Keep its workspace metadata, without importing the desktop or toolkit.
      ../../../desktop/aizome/Cargo.toml
      ../../../desktop/aizome/crates/aizome-ipc
      ../../../desktop/aizome/session/aizome-session
    ];
  };
in
rustPlatform.buildRustPackage {
  pname = "yukimi";
  version = "0.1.0";
  src = source;
  postUnpack = ''
    sourceRoot="$sourceRoot/apps/yukimi"
  '';
  cargoLock.lockFile = ../Cargo.lock;
  nativeBuildInputs = [
    pkg-config
    wrapGAppsHook4
  ];
  buildInputs = [
    gtk4
    libadwaita
    sqlite
  ];
  # Only the window needs GTK's environment. The helper runs as root through
  # pkexec and must be the plain program polkit's policy names.
  dontWrapGApps = true;
  preFixup = ''
    wrapGApp "$out/bin/yukimi"
  '';
  postInstall = ''
    install -Dm644 yukimi/data/io.github.bitemyapp.Yukimi.desktop -t $out/share/applications
    install -Dm644 yukimi/data/io.github.bitemyapp.Yukimi.svg -t $out/share/icons/hicolor/scalable/apps
    # Names this exact helper, so polkit's prompt says what Yukimi is asking for.
    mkdir -p $out/share/polkit-1/actions
    substitute yukimi-helper/data/io.github.bitemyapp.Yukimi.policy.in \
      $out/share/polkit-1/actions/io.github.bitemyapp.Yukimi.policy --subst-var out
  '';
  meta = {
    description = "Snow-viewing for NixOS: see what's installed, find what's available, and change it without editing config";
    homepage = "https://github.com/bitemyapp/sekka/tree/main/apps/yukimi";
    license = with lib.licenses; [
      mit
      asl20
    ];
    mainProgram = "yukimi";
    platforms = lib.platforms.linux;
  };
}
