# SPDX-License-Identifier: MIT OR Apache-2.0
{
  lib,
  rustPlatform,
  pkg-config,
  sqlite,
  util-linux,
  clippy,
}:
let
  # Content-addressed source: the same derivation wherever the checkout is.
  source = lib.fileset.toSource {
    root = ../../..;
    fileset = lib.fileset.unions [
      ../Cargo.toml
      ../Cargo.lock
      ../crates
      ../yukimi-helper
      # Completion detection uses the UI-independent Aizome IPC protocol.
      ../../../desktop/aizome/Cargo.toml
      ../../../desktop/aizome/crates/aizome-ipc
      # Typestate support: `Unsettled` for restore points and the clean-up,
      # `SameTarget` for saved configurations.
      ../../../desktop/aizome/crates/aizome-typestate
      ../../../desktop/aizome/session/aizome-session
    ];
  };
in
rustPlatform.buildRustPackage {
  pname = "yukimi-helper";
  version = "0.1.0";
  src = source;
  postUnpack = ''
    sourceRoot="$sourceRoot/apps/yukimi"
  '';
  cargoLock.lockFile = ../Cargo.lock;
  nativeBuildInputs = [ pkg-config ];
  nativeCheckInputs = [
    util-linux
    clippy
  ];
  # The helper runs programs only through its library, which names the ones
  # it may run; yukimi-helper/clippy.toml denies the other ways to start one.
  # Only that lint fails the build, so a toolchain's new lints don't.
  preCheck = ''
    cargo clippy --offline --locked -p yukimi-helper --all-targets -- \
      -A clippy::all -D clippy::disallowed_methods
  '';
  buildInputs = [ sqlite ];
  postInstall = ''
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
    mainProgram = "sekka-update";
    platforms = lib.platforms.linux;
  };
}
