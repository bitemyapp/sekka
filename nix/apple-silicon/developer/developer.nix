# A developer image only (packages.aarch64-linux.mac-installer-j514s). The
# unmodified Asahi installer, which install-sekka.sh runs, writes no
# per-install handoff: it gives the EFI system partition a fixed volume ID,
# and the root image keeps its label. So this system finds them by those,
# which two such installations visible at once would share, and it has a
# developer account, `sekka` with password `sekka`; change it with `passwd`.
# It is never set up at first boot, and no update puts it into setup.
# Product images have neither (configuration.nix, identity.nix, setup.nix).
{ lib, ... }:
{
  calamares.firstBootSetup.enable = lib.mkForce false;
  calamares.appleSilicon.identity.handoff = lib.mkForce false;
  calamares.installUser = "sekka";
  # What first-boot setup would otherwise choose.
  networking.hostName = "sekka";
  time.timeZone = "UTC";
  i18n.defaultLocale = "en_US.UTF-8";
  services.xserver.xkb.layout = "us";
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
  users.users.sekka = {
    isNormalUser = true;
    description = "Sekka (developer image)";
    extraGroups = [
      "networkmanager"
      "wheel"
    ];
    initialPassword = "sekka";
  };
}
