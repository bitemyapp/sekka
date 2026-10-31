# What tools/frame-benchmark.py needs on a NixOS host without a Cargo
# registry: sway and grim, the observer's compiler and Wayland headers, and
# the two protocol XML files it would otherwise find in the registry.
#
#   nix-shell apps/hotaru/tools/frame-bench-shell.nix --arg nixpkgs /nix/store/…-source
#
# then pass --protocol-xml screencopy=$SCREENCOPY_XML and
# --protocol-xml virtual-keyboard=$VIRTUAL_KEYBOARD_XML.
{
  nixpkgs ? <nixpkgs>,
}:
let
  pkgs = import nixpkgs { };
in
pkgs.mkShell {
  packages = with pkgs; [
    sway
    grim
    dbus
    python3
    util-linux
    gcc
    pkg-config
    wayland
    wayland-scanner
    libxkbcommon
  ];
  SCREENCOPY_XML = "${pkgs.wlr-protocols}/share/wlr-protocols/unstable/wlr-screencopy-unstable-v1.xml";
  VIRTUAL_KEYBOARD_XML = "${pkgs.wlroots.src}/protocol/virtual-keyboard-unstable-v1.xml";
}
