# What every benchmark VM shares: the settings of an installed Sekka system
# (installer/genkan/rust/reference.nix, the reference system the media
# prebuild), adapted to a disposable QEMU guest. Only the desktop differs.
{ config, lib, pkgs, modulesPath, ... }:
{
  imports = [ "${modulesPath}/profiles/qemu-guest.nix" ];

  # As reference.nix: tuning, zswap in front of a RAM-sized swap device,
  # the latest kernel, NetworkManager, printing, PipeWire.
  calamares.tuning.enable = true;
  calamares.zswap.enable = true;
  boot.kernelPackages = pkgs.linuxPackages_latest;
  hardware.cpu.intel.updateMicrocode = true;
  hardware.cpu.amd.updateMicrocode = true;
  hardware.enableRedistributableFirmware = true;
  networking.hostName = "nixos";
  networking.networkmanager.enable = true;
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
  # One person, with a password: the login screen asks for it.
  users.users.demo = {
    isNormalUser = true;
    uid = 1000;
    description = "Demo";
    extraGroups = [
      "networkmanager"
      "wheel"
    ];
    initialPassword = "demo";
  };
  nixpkgs.config.allowUnfree = true;
  system.stateVersion = "26.11";

  # The boot splash's quiet console (boot-splash/default.nix), without
  # Plymouth itself: the splash is the same for every desktop, and its
  # patched build would be compiled for these VMs alone.
  boot.consoleLogLevel = lib.mkDefault 3;
  boot.kernelParams = [
    "quiet"
    "udev.log_level=3"
    "rd.udev.log_level=3"
  ];

  # The VM's disks: an ext4 root, the system's closure as a read-only erofs
  # image (as NixOS's own VM tests' useNixStoreImage), and swap. QEMU boots
  # the kernel directly; there is no boot loader.
  boot.loader.grub.enable = false;
  fileSystems."/" = {
    device = "/dev/disk/by-label/nixos";
    fsType = "ext4";
    options = [ "noatime" ];
  };
  fileSystems."/nix/.ro-store" = {
    device = "/dev/disk/by-label/nix-store";
    fsType = "erofs";
    options = [ "ro" ];
    neededForBoot = true;
  };
  fileSystems."/nix/store" = {
    device = "/nix/.ro-store";
    fsType = "none";
    options = [ "bind" ];
    neededForBoot = true;
    depends = [ "/nix/.ro-store" ];
  };
  swapDevices = [ { device = "/dev/disk/by-label/swap"; } ];
}
