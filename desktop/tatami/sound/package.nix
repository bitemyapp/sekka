# SPDX-License-Identifier: MIT OR Apache-2.0
{
  lib,
  rustPlatform,
  pkg-config,
  wrapGAppsHook4,
  gtk4,
  libadwaita,
  libpulseaudio,
  pulseaudio,
  pipewire,
  coreutils,
  adwaita-icon-theme,
}:
let
  # Content-addressed source, as the helper's (tool/package.nix).
  source = builtins.path {
    path = ./.;
    name = "tatami-sound-src";
    filter =
      path: _:
      !(builtins.elem (baseNameOf path) [
        "target"
        "package.nix"
      ]);
  };
in
rustPlatform.buildRustPackage {
  pname = "tatami-sound";
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
    # The level meter's connection (meter.rs).
    libpulseaudio
  ];
  preFixup = ''
    gappsWrapperArgs+=(--prefix XDG_DATA_DIRS : ${adwaita-icon-theme}/share)
    gappsWrapperArgs+=(--prefix PATH : ${
      lib.makeBinPath [
        pulseaudio
        # pw-metadata, which moves streams (audio.rs).
        pipewire
        coreutils
      ]
    })
  '';
  postInstall = ''
    install -Dm644 data/org.tatami.Sound.desktop -t $out/share/applications
  '';
  meta = {
    description = "The Sound window of the Tatami session";
    license = with lib.licenses; [
      mit
      asl20
    ];
    mainProgram = "tatami-sound";
  };
}
