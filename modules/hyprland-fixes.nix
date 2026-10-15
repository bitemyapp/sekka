# Import from both desktop entry points so standalone Tatami, vanilla
# Hyprland and the installer's reference systems share the same package set.
# NixOS deduplicates this module when both desktop modules are imported.
{ ... }:
{
  nixpkgs.overlays = import ../nix/overlays;
}
