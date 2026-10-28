{ nixpkgs, reference, revision }:
let
  pkgs = import (builtins.toPath nixpkgs) { system = "x86_64-linux"; };
in
pkgs.callPackage (builtins.toPath (reference + "/nix/package.nix")) {
  inherit revision;
  optimize = "fast";
}
