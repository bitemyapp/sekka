# NixOS takes the keyring daemon, its PAM module and its D-Bus service all
# from pkgs.gnome-keyring, so the fix replaces that package for the system.
{ config, lib, ... }:
{
  nixpkgs.overlays = [ (import ../nix/overlays/gnome-keyring.nix) ];

  # PAM starts the unlocked login daemon before its Secret Service is
  # running. Start that service before graphical clients can race D-Bus
  # activation against XDG autostart and launch a second daemon. This is
  # shared by installed desktops and standalone Aizome sessions.
  systemd.user.services.gnome-keyring-secrets = lib.mkIf config.services.gnome.gnome-keyring.enable {
    description = "Secret Service of the login keyring";
    wantedBy = [ "graphical-session-pre.target" ];
    before = [ "graphical-session-pre.target" ];
    partOf = [ "graphical-session-pre.target" ];
    # Only the daemon the password unlocked; without one, the session's
    # autostart entry and D-Bus start one as before.
    unitConfig.ConditionPathExists = "%t/keyring/control";
    serviceConfig = {
      Type = "oneshot";
      RemainAfterExit = true;
      ExecStart = "${config.security.wrapperDir}/gnome-keyring-daemon --start --components=secrets";
    };
  };
}
