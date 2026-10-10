# SPDX-License-Identifier: GPL-3.0-or-later
# A reference installed system for the installation media to prebuild. It
# uses the same static modules and the same closure-relevant settings that
# rust/src/config.rs writes, with placeholder identity and hardware. An actual
# installation then finds almost every store path already on the media and
# only builds small per-machine derivations (configuration files, initrd).
# rust/src/config.rs has a test that keeps the shared settings in sync.
{
  desktops,
  applications ? [ "firefox" ],
  kernel ? "latest",
  filesystem ? "ext4",
  swap ? true,
  tuning ? true,
  stateVersion ? "26.11",
}:
{ lib, pkgs, ... }:
{
  imports = [
    ./src/applications.nix
    ./system
  ];
  calamares.installUser = "reference";
  calamares.applications = applications;
  calamares.desktops = desktops;
  calamares.defaultDesktop = builtins.head desktops;
  calamares.tuning.enable = tuning;
  boot.loader.systemd-boot.enable = true;
  boot.loader.efi.canTouchEfiVariables = true;
  boot.kernelPackages = lib.mkIf (kernel == "latest") pkgs.linuxPackages_latest;
  # Typical detected hardware: both microcode bundles and common storage drivers.
  hardware.cpu.intel.updateMicrocode = true;
  hardware.cpu.amd.updateMicrocode = true;
  boot.initrd.availableKernelModules = [
    "nvme"
    "xhci_pci"
    "ahci"
    "usb_storage"
    "sd_mod"
  ];
  fileSystems."/" = {
    device = "/dev/disk/by-uuid/00000000-0000-4000-8000-000000000000";
    fsType = filesystem;
    options = [ "noatime" ] ++ lib.optional (filesystem == "btrfs") "compress=zstd:1";
  };
  fileSystems."/boot" = {
    device = "/dev/disk/by-uuid/0000-0000";
    fsType = "vfat";
    options = [
      "fmask=0077"
      "dmask=0077"
    ];
  };
  swapDevices = lib.optional swap { device = "/dev/disk/by-uuid/00000000-0000-4000-8000-000000000001"; };
  boot.resumeDevice = lib.mkIf swap "/dev/disk/by-uuid/00000000-0000-4000-8000-000000000001";
  calamares.zswap.enable = swap;
  networking.hostName = "nixos";
  networking.networkmanager.enable = true;
  hardware.enableRedistributableFirmware = true;
  time.timeZone = "UTC";
  i18n.defaultLocale = "en_US.UTF-8";
  services.xserver.xkb.layout = "us";
  console.useXkbConfig = true;
  services.printing.enable = true;
  security.rtkit.enable = true;
  services.pipewire = {
    enable = true;
    alsa.enable = true;
    alsa.support32Bit = true;
    pulse.enable = true;
  };
  users.mutableUsers = true;
  users.users.root.initialHashedPassword = "!";
  users.users.reference = {
    isNormalUser = true;
    extraGroups = [
      "networkmanager"
      "wheel"
    ];
    hashedPasswordFile = "/etc/nixos-secrets/user-password.hash";
  };
  nixpkgs.config.allowUnfree = true;
  system.stateVersion = stateVersion;
}
