{ lib, pkgs, ... }:
{
  nixpkgs.overlays = [ (import ../nix/overlays/pipewire.nix) ];
  services.pipewire.package = lib.mkDefault pkgs.sekka-pipewire-daemon;
  services.pipewire.wireplumber.configPackages = [ (pkgs.callPackage ../nix/audeze-maxwell.nix { }) ];
}
