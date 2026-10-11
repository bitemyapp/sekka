# SPDX-License-Identifier: GPL-3.0-or-later
# Sekka's public binary cache (Cloudflare R2 behind cache.sekka.computer). It
# serves Sekka's own packages and the patched Hyprland, so installations and
# updates download them instead of compiling them. The cache's nix-cache-info
# declares priority 50, so Nix asks cache.nixos.org (40) first. The key only
# vouches for paths signed by Sekka's own cache key.
{ config, lib, ... }:
{
  options.calamares.binaryCache.enable = lib.mkOption {
    type = lib.types.bool;
    default = true;
    description = "Download Sekka's own packages from https://cache.sekka.computer instead of building them.";
  };
  config = lib.mkIf config.calamares.binaryCache.enable {
    nix.settings = {
      extra-substituters = [ "https://cache.sekka.computer" ];
      extra-trusted-public-keys = [
        "cache.sekka.computer-1:JJsOkmRt3iCwjvJrol8x2a7euU+kMV7iHaDSFzofA+Y="
      ];
    };
  };
}
