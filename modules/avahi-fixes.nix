# Both live media and installed desktop selections use the same daemon fix.
{ lib, pkgs, ... }:
{
  nixpkgs.overlays = [ (import ../nix/overlays/avahi.nix) ];
  services.avahi.package = lib.mkDefault pkgs.sekka-avahi-daemon;
}
