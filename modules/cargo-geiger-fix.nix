{
  nixpkgs.overlays = [ (import ../nix/overlays/cargo-geiger.nix) ];
}
