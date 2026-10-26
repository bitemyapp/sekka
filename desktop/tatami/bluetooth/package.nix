# SPDX-License-Identifier: MIT OR Apache-2.0
{
  lib,
  rustPlatform,
  pkg-config,
  wrapGAppsHook4,
  gtk4,
  libadwaita,
  dbus,
  xvfb-run,
}:
let
  # Content-addressed source, as the helper's (tool/package.nix).
  source = builtins.path {
    path = ./.;
    name = "tatami-bluetooth-src";
    filter =
      path: _:
      !(builtins.elem (baseNameOf path) [
        "target"
        "package.nix"
      ]);
  };
in
rustPlatform.buildRustPackage {
  pname = "tatami-bluetooth";
  version = "0.1.0";
  src = source;
  cargoLock.lockFile = "${source}/Cargo.lock";
  nativeBuildInputs = [
    pkg-config
    wrapGAppsHook4
  ];
  buildInputs = [
    gtk4
    libadwaita
  ];
  nativeCheckInputs = [
    dbus
    xvfb-run
  ];
  checkPhase = ''
    runHook preCheck
    export GDK_BACKEND=x11 GSK_RENDERER=cairo GTK_A11Y=none GIO_USE_VFS=local
    xvfb-run -a dbus-run-session --config-file=data/test-bus.conf -- cargo test --offline --locked --release -- --include-ignored
    runHook postCheck
  '';
  postInstall = ''
    install -Dm644 data/org.tatami.Bluetooth.desktop -t $out/share/applications
  '';
  meta = {
    description = "The Bluetooth window of the Tatami session";
    license = with lib.licenses; [
      mit
      asl20
    ];
    mainProgram = "tatami-bluetooth";
  };
}
