# Tools and subjects for the terminal performance harness
# (docs/performance/terminals.md). Use the Nixpkgs Sekka itself pins, so the
# terminals' versions are the ones a Sekka system installs:
#
#   nix-shell tests/performance/terminals/shell.nix \
#     --arg nixpkgs "$(nix eval --raw --inputs-from . nixpkgs#path)" \
#     --argstr hotaru "$(nix build --no-link --print-out-paths .#hotaru)" \
#     --argstr fonts "$(nix build --no-link --print-out-paths .#aizome-fonts)"
#
# The harness gets every subject and protocol file from the environment
# variables below, and records their store paths in each result.
{
  nixpkgs ? <nixpkgs>,
  hotaru ? null,
  fonts ? null,
}:
let
  pkgs = import nixpkgs { };
in
pkgs.mkShell {
  packages = with pkgs; [
    sway
    grim
    dbus
    dconf
    python3
    util-linux
    procps
    gcc
    pkg-config
    wayland
    wayland-scanner
    libxkbcommon
    xz
  ];
  SCREENCOPY_XML = "${pkgs.wlr-protocols}/share/wlr-protocols/unstable/wlr-screencopy-unstable-v1.xml";
  VIRTUAL_KEYBOARD_XML = "${pkgs.wlroots.src}/protocol/virtual-keyboard-unstable-v1.xml";
  TERMBENCH_HOTARU = if hotaru == null then "" else "${hotaru}/bin/hotaru";
  TERMBENCH_FONTS = if fonts == null then "" else fonts;
  TERMBENCH_GHOSTTY = "${pkgs.ghostty}/bin/ghostty";
  TERMBENCH_GNOME_TERMINAL = "${pkgs.gnome-terminal}";
  TERMBENCH_KGX = "${pkgs.gnome-console}/bin/kgx";
  TERMBENCH_KONSOLE = "${pkgs.kdePackages.konsole}/bin/konsole";
  TERMBENCH_FOOT = "${pkgs.foot}/bin/foot";
  TERMBENCH_ALACRITTY = "${pkgs.alacritty}/bin/alacritty";
  TERMBENCH_KITTY = "${pkgs.kitty}/bin/kitty";
  TERMBENCH_DCONF_SERVICE = "${pkgs.dconf}/libexec/dconf-service";
}
