# Component check of the actual IBus callback, including the failing original.
{ pkgs }:
pkgs.runCommand "ibus-wayland-serial"
  {
    nativeBuildInputs = [
      pkgs.stdenv.cc
      pkgs.gawk
      pkgs.patch
    ];
  }
  ''
    cp -r ${pkgs.ibus.src} source
    chmod -R u+w source
    cd source
    patch --fuzz=0 -p1 < ${./patches/ibus-wayland-forward-keycode.patch}
    cp client/wayland/ibuswaylandim.c before.c
    patch --fuzz=0 -p1 < ${./patches/ibus-wayland-serial.patch}
    # Keep the harness beside its driver, as in the checkout.
    cp ${../tests/check-ibus-wayland-serial} check-ibus-wayland-serial
    cp ${../tests/ibus-wayland-serial.c} ibus-wayland-serial.c
    bash check-ibus-wayland-serial before.c client/wayland/ibuswaylandim.c
    touch "$out"
  ''
