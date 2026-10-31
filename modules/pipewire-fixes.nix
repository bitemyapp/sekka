{ lib, pkgs, ... }:
{
  nixpkgs.overlays = [ (import ../nix/overlays/pipewire.nix) ];
  services.pipewire.package = lib.mkDefault pkgs.sekka-pipewire-daemon;
}
