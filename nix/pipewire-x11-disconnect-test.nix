{ pkgs, patched }:
pkgs.runCommand "sekka-pipewire-x11-disconnect"
  {
    nativeBuildInputs = [ pkgs.python3 ];
  }
  ''
    python3 ${../tests/check_pipewire_x11_disconnect.py} \
      --pipewire ${patched}/bin/pipewire \
      --xvfb ${pkgs.xorg-server}/bin/Xvfb \
      --output "$out"
  ''
