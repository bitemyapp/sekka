# Compare the same runtime roots across packaging revisions. Use a fresh,
# distinct version per cold build: shared dependency derivations would
# otherwise make a later candidate inherit an earlier candidate's warm cache.
{ sourceRef, version }:
let
  f = builtins.getFlake sourceRef;
  a = import (f.outPath + "/desktop/aizome/nix") (
    {
      inherit version;
      inherit (f.inputs) nixpkgs;
    }
    // (if f.inputs ? crane then { inherit (f.inputs) crane; } else { })
  );
  pkgs = f.inputs.nixpkgs.legacyPackages.x86_64-linux;
in
pkgs.linkFarm "aizome-comparison" [
  {
    name = "aizome";
    path = a.packages.x86_64-linux.aizome;
  }
  {
    name = "hotaru";
    path = a.packages.x86_64-linux.hotaru;
  }
]
