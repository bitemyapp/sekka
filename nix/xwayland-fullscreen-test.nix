{
  pkgs,
  aizome,
  satellite,
}:
pkgs.runCommand "aizome-xwayland-fullscreen"
  {
    nativeBuildInputs = [
      pkgs.stdenv.cc
      pkgs.pkg-config
      pkgs.python3
    ];
    buildInputs = [
      pkgs.libx11
      pkgs.libxrandr
    ];
    AIZOME = "${aizome}/bin/aizome";
    AIZOMECTL = "${aizome}/bin/aizomectl";
    SATELLITE = "${satellite}/bin/xwayland-satellite";
  }
  ''
    cc -Wall -Wextra -Werror ${../tests/x11-fullscreen-client.c} \
      $(pkg-config --cflags --libs x11 xrandr) -o x11-fullscreen-client
    export X11_FIXTURE="$PWD/x11-fullscreen-client"
    python3 ${../tests/test_xwayland_fullscreen.py}
    touch "$out"
  ''
