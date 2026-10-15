# Sekka's fixes to the Hyprland stack, in order. The flake's package set,
# overlays.default and installed systems (modules/hyprland-fixes.nix) all
# use this list.
[
  (import ./aquamarine.nix)
  (import ./hyprland.nix)
]
