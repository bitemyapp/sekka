# Shared by installed desktop selections and the live media, whose boot
# entries all run Aizome but would get the patched portal with Xfce.
{ ... }:
{
  nixpkgs.overlays = [ (import ../nix/overlays/xapp.nix) ];
}
