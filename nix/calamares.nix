# Genkan's package retains its existing executable and policy IDs for compatibility.
# Its window builds against Aizome's kit and Sukumo, and its tests draw
# with Aizome's fonts.
{
  pkgs,
  source,
  fonts,
}:
let
  # Import through a path when the source is one (this checkout's
  # installer/genkan): interpolating it into a string would copy it to the
  # store first, which `nix flake check --no-build` can't do, so the check
  # would fail wherever that copy isn't already in the store.
  root = source.outPath or source;
in
import (if builtins.isPath root then root + "/nix/package.nix" else "${root}/nix/package.nix") {
  inherit pkgs fonts;
}
