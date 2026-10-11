# SPDX-License-Identifier: GPL-3.0-or-later
# The installed system's desktop, swap and tuning modules, exported by this
# repository's flake as nixosModules.desktops (and, with the application
# catalog, nixosModules.default). Installed systems import them from their
# flake's `calamares` input. The installation media evaluate the same files
# for their prebuilt reference systems, so an installation reuses packages
# already on the media.
{ pkgs, ... }:
{
  environment.systemPackages = [ pkgs.file ];
  imports = [
    ./binary-cache.nix
    ./desktops.nix
    ./nvidia.nix
    ./tuning.nix
    ./usb-wake.nix
  ];
}
