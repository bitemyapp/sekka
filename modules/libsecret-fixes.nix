{
  nixpkgs.overlays = [ (import ../nix/overlays/libsecret.nix) ];
}
