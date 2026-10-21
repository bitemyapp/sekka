# Shared by installed desktop selections and the live compatibility session.
{ ... }:
{
  nixpkgs.overlays = [ (import ../nix/overlays/xapp.nix) ];
}
