# Experimental live media for unlocked x86_64 Googlebooks with Depthcharge.
# Genkan cannot yet install their boot format; do not offer disk installation.
{ config, lib, ... }:
{
  respin.enableInstaller = false;
  respin.referenceDesktops = lib.mkForce [ ];
  respin.cacheApplications = false;
  specialisation = lib.mkForce { };

  # The coreboot framebuffer is not an EFI framebuffer. Make its discovery
  # available before Plymouth, including when the Intel GPU has no driver yet.
  boot.initrd.kernelModules = [
    "coreboot_table"
    "framebuffer_coreboot"
  ];
  services.xserver.videoDrivers = lib.mkForce [
    "modesetting"
    "fbdev"
  ];

  isoImage = {
    volumeID = lib.mkForce "SEKKA_GOOGLEBOOK";
    # PC isohybrid GPT entries overlap, which vboot rejects. googlebook-iso.nix
    # makes a disjoint ISO9660 data partition and a native vboot kernel partition.
    makeBiosBootable = lib.mkForce false;
    makeEfiBootable = lib.mkForce false;
    makeUsbBootable = lib.mkForce false;
  };
  # More specific than the PC live module's forced name (priority 50).
  image.baseName = lib.mkOverride 40 "sekka-googlebook-${config.system.nixos.label}-x86_64-linux";
}
