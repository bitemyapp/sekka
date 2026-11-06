{ pkgs }:
let
  source = pkgs.fetchurl {
    url = "https://github.com/omacom/linux-aurora/releases/download/aurora-2026.10.17.20/aurora-linux-c46d7dcdb8712ea9aeb0ce47b439ffae76371e55.tar.gz";
    hash = "sha256-ST7Z+0qeJF/VmpKiOW08loUWvpMnSbgFl1eik4BlJQk=";
  };
in
pkgs.runCommand "t6030-backlight-backport"
  {
    nativeBuildInputs = [
      pkgs.python3
      pkgs.stdenv.cc
      pkgs.gnutar
      pkgs.gzip
      pkgs.patch
    ];
  }
  ''
    tar xf ${source} --strip-components=1 --wildcards '*/drivers/gpu/drm/apple/*'
    chmod -R u+w drivers
    patch --batch --fuzz=0 -p1 < ${./patches/t6030-panel-backlight.patch}
    python3 ${../../tests/test_apple_backlight.py} drivers/gpu/drm/apple
    touch $out
  ''
