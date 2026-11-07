# Written by Sekka's first-boot setup: what was chosen when this computer
# was set up. configuration.nix imports it. Change it as you would the rest
# of the configuration; the next update builds it.
{ lib, ... }:
{
  calamares.firstBootSetup.applied = true;
  calamares.installUser = "ada";
  calamares.defaultDesktop = "aizome";
  users.users."ada" = {
    isNormalUser = true;
    uid = 1000;
    description = "Ada Lovelace";
    extraGroups = [ "networkmanager" "wheel" ];
    # The password is the one set at setup, in /etc/shadow
    # (users.mutableUsers keeps it); change it with passwd or in Settings.
  };
  networking.hostName = "analytical";
  services.xserver.xkb.layout = "gb";
  # The language, en_GB.UTF-8, and the time zone, Europe/London, are
  # the system's own settings: Settings changes them, and updates keep
  # them. The line below only seeds /etc/locale.conf, should it ever be
  # missing.
  i18n.defaultLocale = "en_GB.UTF-8";
  # This installation's own partitions, as the Mac installer handed them
  # over: by GPT partition UUID, which no other installation shares.
  fileSystems."/".device = lib.mkForce "/dev/disk/by-partuuid/63d89be1-72f7-4aff-9679-09cafa7e0c2d";
  fileSystems."/boot".device = lib.mkForce "/dev/disk/by-partuuid/910de5ca-7390-4716-9a54-8028916e4d78";
}
