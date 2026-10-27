# Aizome's login screen (docs/greeter.md): greetd runs aizome-greeter as the
# greeter user; it starts Aizome's compositor for itself, without the
# shell, and lists every session the system has. greetd's PAM service
# includes `login`, which unlocks GNOME Keyring with the password.
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.programs.aizome;
  g = cfg.greeter;
  toml = pkgs.formats.toml { };
  xkb = config.services.xserver.xkb;
  # The compositor's settings at the login screen: the system's keyboard.
  compositorSettings = toml.generate "aizome-greeter-compositor.toml" {
    input.keyboard = {
      layouts = lib.splitString "," xkb.layout;
      inherit (xkb) variant options model;
    };
  };
  # The desktop Exec alone skips NixOS's environment/session setup. Keep a
  # parent around too: upstream X11 entries such as startxfce4 do not stop
  # the fake graphical target on exit, and the user manager may outlive them.
  x11Session = pkgs.writeShellScript "aizome-x11-session" ''
    cleanup() {
      # A portal's Requisite can keep graphical-session.target needed even
      # after the fake target stops. End the graphical session explicitly.
      ${config.systemd.package}/bin/systemctl --user stop nixos-fake-graphical-session.target graphical-session.target
      ${config.systemd.package}/bin/systemctl --user unset-environment DISPLAY XAUTHORITY XDG_SESSION_ID XDG_CURRENT_DESKTOP XDG_SESSION_DESKTOP XDG_SESSION_TYPE
    }
    trap cleanup EXIT
    ${config.services.displayManager.sessionData.wrapper} "$@"
  '';
  greeterConfig = toml.generate "aizome-greeter.toml" (
    {
      sessions = g.sessionDirs;
      hidden_sessions = g.hiddenSessions;
      # startx runs the X server with NixOS's arguments, on the session's
      # own VT.
      x11_prefix = lib.optionals g.x11 [
        "startx"
        "${x11Session}"
      ];
      hidden_users = config.services.displayManager.hiddenUsers;
      inherit (g) power;
      compositor = [
        "${cfg.package}/bin/aizome"
        "--backend"
        "udev"
        "--no-shell"
        "--no-xwayland"
        "--no-session"
      ];
      compositor_settings = "${compositorSettings}";
    }
    // lib.optionalAttrs (g.defaultSession != null) { default_session = g.defaultSession; }
    // lib.optionalAttrs (cfg.wallpapers.night != null) { wallpaper_night = "${cfg.wallpapers.night}"; }
    // lib.optionalAttrs (cfg.wallpapers.day != null) { wallpaper_day = "${cfg.wallpapers.day}"; }
  );
  autoLogin = config.services.displayManager.autoLogin;
in
{
  options.programs.aizome.greeter = {
    enable = lib.mkEnableOption ''
      Aizome's login screen: greetd, with aizome-greeter on Aizome's own
      compositor. It lists every session the system has (Aizome, other
      desktops and window managers) and remembers each person's last one'';
    sessionDirs = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [
        "${config.services.displayManager.sessionData.desktops}/share/wayland-sessions"
        "${config.services.displayManager.sessionData.desktops}/share/xsessions"
      ];
      defaultText = lib.literalExpression ''
        [
          "''${config.services.displayManager.sessionData.desktops}/share/wayland-sessions"
          "''${config.services.displayManager.sessionData.desktops}/share/xsessions"
        ]
      '';
      description = "Where the sessions to offer are (a directory named `xsessions` holds X11 ones).";
    };
    hiddenSessions = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ ];
      example = [ "hyprland" ];
      description = "Sessions not to offer, by their desktop file's name without `.desktop`.";
    };
    defaultSession = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = config.services.displayManager.defaultSession;
      defaultText = lib.literalExpression "config.services.displayManager.defaultSession";
      description = "The session picked for someone who hasn't logged in on this computer before.";
    };
    x11 = lib.mkOption {
      type = lib.types.bool;
      default = config.services.xserver.enable;
      defaultText = lib.literalExpression "config.services.xserver.enable";
      description = "Offer X11 sessions, started with startx and NixOS's X server arguments.";
    };
    power = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Offer Sleep, Restart and Shut Down on the login screen.";
    };
  };

  config = lib.mkIf (cfg.enable && g.enable) {
    environment.etc."aizome/greeter.toml".source = greeterConfig;
    services.greetd = {
      enable = true;
      # From the running system rather than by store path, as Sekka starts
      # Tatami: an Aizome update then leaves greetd's configuration (and
      # unit) as it was, so it applies live, and the next login screen is
      # the new greeter. By store path, every update would change greetd's
      # configuration, which waits for a restart.
      settings.default_session = {
        command = "/run/current-system/sw/bin/aizome-greeter";
        user = "greeter";
      };
      # Logging in by itself at boot: straight into the default session,
      # without the greeter (greetd's initial session).
      settings.initial_session = lib.mkIf (autoLogin.enable && autoLogin.user != null) {
        command = "/run/current-system/sw/bin/aizome-greeter --session ${
          if g.defaultSession != null then g.defaultSession else "aizome"
        }";
        inherit (autoLogin) user;
      };
    };
    services.xserver.displayManager.startx.enable = lib.mkIf g.x11 true;
    # Who logged in last and each person's session (nothing secret).
    systemd.tmpfiles.rules = [ "d /var/lib/aizome-greeter 0700 greeter greeter -" ];
  };
}
