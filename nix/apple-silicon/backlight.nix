# Optional backport for the Apple Silicon developer image's pinned kernel.
# Drop this module when that image moves to an upstream kernel with PR #183.
{ config, pkgs, ... }:
{
  assertions = [
    {
      assertion =
        pkgs.stdenv.hostPlatform.system == "aarch64-linux"
        && config.boot.kernelPackages.kernel.modDirVersion == "7.1.12-aurora20"
        &&
          config.boot.kernelPackages.kernel.src.url
          == "https://github.com/omacom/linux-aurora/releases/download/aurora-2026.10.17.20/aurora-linux-c46d7dcdb8712ea9aeb0ce47b439ffae76371e55.tar.gz";
      message = "Sekka's T6030 backlight backport requires the pinned aurora-2026.10.17.20 kernel; remove this module when upgrading it.";
    }
  ];

  boot.kernelPatches = [
    {
      name = "t6030-panel-backlight";
      patch = ./patches/t6030-panel-backlight.patch;
    }
  ];
}
