# Resolve the evaluated session defaults with xdg-utils, without a desktop
# or an Aizome build. Launch recorders stand in for the two player binaries.
{
  pkgs,
  inputs,
  installer,
}:
let
  config =
    (inputs.nixpkgs.lib.nixosSystem {
      system = pkgs.stdenv.hostPlatform.system;
      modules = [
        inputs.aizome.nixosModules.default
        { programs.aizome.enable = true; }
      ];
    }).config;
  defaults = config.environment.etc."xdg/aizome-mimeapps.list".text;
in
assert installer.config.environment.etc."xdg/aizome-mimeapps.list".text == defaults;
pkgs.runCommand "aizome-media-defaults"
  {
    nativeBuildInputs = [
      pkgs.python3
      pkgs.xdg-utils
      pkgs.shared-mime-info
      pkgs.desktop-file-utils
    ];
    defaultsFile = pkgs.writeText "aizome-mimeapps.list" defaults;
    desktopFile = ../desktop/aizome/apps/utsushi/data/org.aizome.Utsushi.desktop;
    videoFile = ../desktop/aizome/crates/aizome-media/tests/fixtures/clip.mp4;
    musicFile = ../desktop/aizome/crates/aizome-media/tests/fixtures/song.flac;
    mimeData = "${pkgs.shared-mime-info}/share";
    runtimeShell = pkgs.runtimeShell;
  }
  ''
    python ${./aizome-media-defaults-test.py}
    touch "$out"
  ''
