# SPDX-License-Identifier: MIT OR Apache-2.0
# One interface, from Aizome's workspace, plus the independently built helper.
# The fallback supports the existing non-flake NixOS module with this checkout's
# pinned external Sidestep; first-party sources always remain local.
{
  pkgs,
  nativeYukimi ?
    let
      locked = (builtins.fromJSON (builtins.readFile ../../../flake.lock)).nodes.sidestep.locked;
      aizome = import ../../../desktop/aizome/nix {
        nixpkgs = {
          inherit (pkgs) lib;
          legacyPackages.${pkgs.stdenv.hostPlatform.system} = pkgs;
        };
        sidestep = builtins.fetchTree locked;
      };
    in
    aizome.packages.${pkgs.stdenv.hostPlatform.system}.aizome.yukimi,
}:
let
  helper = pkgs.callPackage ./helper.nix { };
in
pkgs.symlinkJoin {
  name = "yukimi-0.1.0";
  paths = [
    nativeYukimi
    helper
  ];
  passthru = { inherit helper nativeYukimi; };
  meta = helper.meta // {
    description = "Yukimi, Sekka's software manager on Aizome's native toolkit";
    mainProgram = "yukimi";
  };
}
