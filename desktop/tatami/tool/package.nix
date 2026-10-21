# SPDX-License-Identifier: MIT OR Apache-2.0
{
  lib,
  rustPlatform,
  pkg-config,
  makeWrapper,
  gdk-pixbuf,
  gnome,
  librsvg,
  webp-pixbuf-loader,
  libjxl,
}:
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
  nativeBuildInputs = [
    pkg-config
    makeWrapper
  ];
  buildInputs = [ gdk-pixbuf ];
  # Match swaybg's decoder and extra formats. Validate full image data before
  # replacing a saved choice, not just a filename, signature or thumbnail.
  postFixup = ''
    wrapProgram $out/bin/tatami --set GDK_PIXBUF_MODULE_FILE ${
      gnome._gdkPixbufCacheBuilder_DO_NOT_USE {
        extraLoaders = [
          librsvg
          webp-pixbuf-loader
          libjxl
        ];
      }
    }
  '';
  meta = {
    description = "Session launcher and desktop commands for Tatami";
    license = with lib.licenses; [
      mit
      asl20
    ];
    mainProgram = "tatami";
  };
}
