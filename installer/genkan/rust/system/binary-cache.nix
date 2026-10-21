# SPDX-License-Identifier: GPL-3.0-or-later
# Sekka's public binary cache (Cloudflare R2 behind cache.sekka.computer). It
# serves Sekka's own packages and the patched Hyprland, so installations and
# updates download them instead of compiling them. The cache's nix-cache-info
# declares priority 50, so Nix asks cache.nixos.org (40) first. The key only
# vouches for paths signed by Sekka's own cache key.
#
# The catalog's AI applications come from numtide/llm-agents.nix, newer than
# Nixpkgs' and not on cache.nixos.org; numtide's own cache serves them as
# built from that flake's lock, which installed systems keep. Its
# nix-cache-info says priority 30; asking it last (55), for what neither
# cache.nixos.org nor Sekka's cache has, spares every other path a request
# there.
{ config, lib, ... }:
{
  options.calamares.binaryCache = {
    enable = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Download Sekka's own packages from https://cache.sekka.computer instead of building them.";
    };
    numtide.enable = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Download the AI applications (numtide/llm-agents.nix) from https://cache.numtide.com instead of building them.";
    };
    determinate.enable = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Download the pinned Determinate Nix bootstrap packages from its signed public cache.";
    };
  };
  config = lib.mkMerge [
    (lib.mkIf config.calamares.binaryCache.enable {
      nix.settings = {
        extra-substituters = [ "https://cache.sekka.computer" ];
        extra-trusted-public-keys = [
          "cache.sekka.computer-1:JJsOkmRt3iCwjvJrol8x2a7euU+kMV7iHaDSFzofA+Y="
        ];
      };
    })
    (lib.mkIf config.calamares.binaryCache.numtide.enable {
      nix.settings = {
        extra-substituters = [ "https://cache.numtide.com?priority=55" ];
        extra-trusted-public-keys = [
          "niks3.numtide.com-1:DTx8wZduET09hRmMtKdQDxNNthLQETkc/yaX7M4qK0g="
        ];
      };
    })
    (lib.mkIf config.calamares.binaryCache.determinate.enable {
      nix.settings = {
        extra-substituters = [ "https://install.determinate.systems" ];
        extra-trusted-public-keys = [
          "cache.flakehub.com-3:hJuILl5sVK4iKm86JzgdXW12Y2Hwd5G07qKtHTOcDCM="
        ];
      };
    })
  ];
}
