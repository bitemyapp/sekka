{ config, pkgs, ... }: {
  calamares = {
    installUser = "sekka";
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
    };
  };
  boot.loader.systemd-boot.enable = true;

  boot.loader.systemd-boot.configurationLimit = 5;
  fileSystems."/" = {
    device = "/dev/disk/by-label/sekka-root";
    fsType = "ext4";
    options = [ "noatime" ];
    # The installer grows the partition (expand); this grows ext4.
    autoResize = true;
  };
  fileSystems."/boot" = {
    device = "/dev/disk/by-uuid/5ECC-A5E1";
    fsType = "vfat";
    options = [
      "fmask=0077"
      "dmask=0077"
    ];
  };
  networking.hostName = "sekka";
  networking.networkmanager.enable = true;
  hardware.enableRedistributableFirmware = true;
  time.timeZone = "UTC";
  i18n.defaultLocale = "en_US.UTF-8";
  services.xserver.xkb.layout = "us";
  console.useXkbConfig = true;
  services.printing.enable = true;
  services.pipewire = {
    enable = true;
    alsa.enable = true;
    pulse.enable = true;
  };
  users.mutableUsers = true;
  users.users.sekka = {
    isNormalUser = true;
    description = "Sekka";
    extraGroups = [
      "networkmanager"
      "wheel"
    ];
  };
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
