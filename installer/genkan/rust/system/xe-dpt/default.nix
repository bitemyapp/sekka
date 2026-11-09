# SPDX-License-Identifier: GPL-3.0-or-later
# Linux's xe driver (Intel graphics from Lunar Lake on) fills a display page
# table for every tiled framebuffer it flips to, looking each 4 KiB page up
# from the start of the buffer's scatterlist. Once memory is fragmented,
# that costs milliseconds of the compositor's CPU on every page flip.
# Until a kernel with the fix reaches Nixpkgs (docs/upstream/linux), Sekka
# replaces the xe module alone, for the kernel the patch was written for;
# the kernel itself stays the cached Nixpkgs build (DEVELOPERS.md, "Intel
# page flips").
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.calamares.xeDpt;
  kernel = config.boot.kernelPackages.kernel;
in
{
  options.calamares.xeDpt.enable = lib.mkOption {
    type = lib.types.bool;
    default = true;
    description = ''
      Whether to replace Linux's xe module with one that fills display page
      tables in a single pass. Only Linux 7.3 gets it; other kernels keep
      their own xe. The replacement requires modular xe, no randomized
      structure layouts, and no mandatory module signatures. Disable this
      option for kernels with those settings.
    '';
  };
  config = lib.mkIf (cfg.enable && lib.versions.majorMinor kernel.version == "7.3") {
    boot.extraModulePackages = [ (pkgs.callPackage ./package.nix { inherit kernel; }) ];
  };
}
