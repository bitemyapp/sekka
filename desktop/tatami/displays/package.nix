# SPDX-License-Identifier: MIT OR Apache-2.0
{
  lib,
  rustPlatform,
  pkg-config,
  wrapGAppsHook4,
  gtk4,
  libadwaita,
}:
let
  # Content-addressed source, as the helper's (tool/package.nix).
  source = builtins.path {
    path = ./.;
    name = "tatami-displays-src";
    filter =
      path: _:
      !(builtins.elem (baseNameOf path) [
        "target"
        "package.nix"
      ]);
  };
in
rustPlatform.buildRustPackage {
  pname = "tatami-displays";
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
  postInstall = ''
    install -Dm644 data/org.tatami.Displays.desktop -t $out/share/applications
  '';
  meta = {
    description = "The Displays window of the Tatami session";
    license = with lib.licenses; [
      mit
      asl20
    ];
    mainProgram = "tatami-displays";
  };
}
