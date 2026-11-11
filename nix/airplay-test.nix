{ pkgs, doubletake }:
pkgs.runCommand "sekka-airplay-media"
  {
    nativeBuildInputs = [ pkgs.python3 ];
    FONTCONFIG_FILE = pkgs.makeFontsConf { fontDirectories = [ pkgs.dejavu_fonts ]; };
  }
  ''
    export XDG_CACHE_HOME=$TMPDIR/cache
    mkdir -p "$XDG_CACHE_HOME"
    python3 ${../tests/check_airplay.py} \
      ${doubletake}/bin/doubletake ${doubletake}/bin/doubletake-test-receiver
    touch $out
  ''
