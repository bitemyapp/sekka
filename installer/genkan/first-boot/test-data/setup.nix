# Written by Sekka's first-boot setup: what was chosen when this computer
# was set up. configuration.nix imports it. Change it as you would the rest
# of the configuration; the next update builds it.
{ lib, ... }:
{
  calamares.firstBootSetup.applied = true;
  calamares.installUser = "ada";
  # Over the image's default, and under a plain value in configuration.nix,
  # which is where Yukimi changes the default desktop.
  calamares.defaultDesktop = lib.mkOverride 900 "tatami";
  users.users."ada" = {
    isNormalUser = true;
    uid = 1000;
    description = "Ada Lovelace";
    extraGroups = [ "networkmanager" "wheel" ];
    # The shell it was made with (users.defaultUserShell), by its path: the
    # system has it already, and naming its package again would rebuild the
    # system's packages for nothing.
    shell = "/run/current-system/sw/bin/bash";
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
}
