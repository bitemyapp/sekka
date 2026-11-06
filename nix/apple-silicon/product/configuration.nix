# Sekka on a 14-inch MacBook Pro with an M3 Pro (J514S). Sekka's Mac image
# was built from exactly this file with the flake.nix and flake.lock beside
# it, so rebuilding them unchanged gives the system the image installed.
#
# Files beside this one that are imported when they exist:
# - identity.nix: this installation's own root and EFI system partition,
#   written once at the first start from the installer's handoff
#   (sekka-install-identity.service);
# - setup.nix: the owner's account and choices, written by first-boot setup;
# - developer.nix: only in developer images (the `sekka` account and the
#   Asahi installer's fixed partition labels).
{ lib, pkgs, ... }:
{
  imports = builtins.filter builtins.pathExists [
    ./identity.nix
    ./setup.nix
    ./developer.nix
  ];

  calamares = {
    applications = [ "firefox" ];
    desktops = [
      "aizome"
      "tatami"
    ];
    defaultDesktop = "aizome";
    loginScreen = "aizome";
    tuning.enable = true;
    bootSplash.enable = true;
    appleSilicon = {
      enable = true;
      t6030Handoff = true;
      # Installed from Sekka's Mac image: its Nix store is registered at the
      # first start, and the root and EFI system partition are the ones the
      # installer named on the kernel command line (sekka.root=, sekka.esp=)
      # until identity.nix names them.
      image.enable = true;
      identity.handoff = true;
    };
  };

  # FIRST-BOOT SETUP. Once Sekka's first-boot setup module is part of Sekka,
  # enable it here, and only here:
  #   calamares.firstBootSetup.enable = true;

  boot.loader.systemd-boot.enable = true;
  boot.loader.systemd-boot.configurationLimit = 5;
  # QEMU's virt machine reaches its disks through virtio-mmio, so this same
  # system boots in a test VM; on a Mac the module never loads.
  boot.initrd.availableKernelModules = [ "virtio_mmio" ];

  # Defaults that setup.nix replaces with the owner's choices. Genkan's
  # modules name the account it created; until setup creates the owner's,
  # that is root, which has no password.
  calamares.installUser = lib.mkDefault "root";
  networking.hostName = lib.mkDefault "sekka";
  time.timeZone = lib.mkDefault "UTC";
  i18n.defaultLocale = lib.mkDefault "en_US.UTF-8";
  services.xserver.xkb.layout = lib.mkDefault "us";

  networking.networkmanager.enable = true;
  hardware.enableRedistributableFirmware = true;
  console.useXkbConfig = true;
  services.printing.enable = true;
  services.pipewire = {
    enable = true;
    alsa.enable = true;
    pulse.enable = true;
  };
  # No account and no usable password until setup creates the owner's.
  users.mutableUsers = true;
  users.users.root.initialHashedPassword = "!";
  nixpkgs.config.allowUnfree = true;
  system.stateVersion = "26.11";

  zramSwap.enable = true;
  nix.settings.max-jobs = 2;
  nix.settings.cores = 4;
  environment.systemPackages = [
    pkgs.git
    pkgs.python3
  ];
}
