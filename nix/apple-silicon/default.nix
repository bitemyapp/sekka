# Sekka's packages for Apple Silicon Macs (aarch64-linux): its own components,
# built as they are for PCs, and the platform Omarchy's release
# aurora-2026.10.17.20 was reported working with on a 14-inch M3 Pro (J514S):
# the linux-aurora kernel, m1n1-aurora and the M3 Pro's Mesa.
{
  inputs,
  nixpkgs,
  hyprlandOverlay,
}:
let
  system = "aarch64-linux";
  pkgs = nixpkgs.legacyPackages.${system};
  hyprlandPkgs = pkgs.extend hyprlandOverlay;
  kernel = pkgs.callPackage ./kernel.nix { };
  mesa = pkgs.callPackage ./mesa.nix { };
in
{
  genkan = import ../calamares.nix {
    inherit pkgs;
    source = inputs.calamares;
  };
  tools = import ../tools.nix { inherit pkgs; };
  inherit (inputs.tatami.packages.${system})
    tatami
    displays
    network
    activity
    wallpapers
    ;
  inherit (inputs.yukimi.packages.${system}) yukimi;
  inherit (inputs.kakehashi.packages.${system}) kakehashi;
  inherit (hyprlandPkgs) aquamarine hyprland;

  linux-aurora = kernel.kernel;
  m1n1-aurora = pkgs.callPackage ./m1n1.nix { };
  mesa-m3-native-pro = mesa.native;
  mesa-m3-pro-vulkan = mesa.vulkan;
}
