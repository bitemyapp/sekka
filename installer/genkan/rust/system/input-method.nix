# IBus is also installed when GNOME is one of several chosen desktops.
{
  config,
  lib,
  pkgs,
  ...
}:
let
  im = config.i18n.inputMethod;
  desktops = config.calamares.desktops;
  has = name: builtins.elem name desktops;
  aizome = has "aizome";
  hyprland = has "hyprland" || has "tatami" || has "omarchy";
  # An explicitly configured panel belongs to the user's own integration.
  wayland = aizome && im.ibus.panel == null;
  # Only Aizome's frontend needs this backport. Keep the distribution's
  # daemon, configured engines and GTK/Qt modules for the other desktops.
  # Remove once pinned IBus includes upstream 58b74bd76d79.
  frontend = pkgs.ibus.overrideAttrs (old: {
    patches = (old.patches or [ ]) ++ [ ../../../../nix/patches/ibus-wayland-forward-keycode.patch ];
  });
  startFrontend = pkgs.writeShellScript "sekka-aizome-ibus" ''
    ${frontend}/libexec/ibus-ui-gtk3 --enable-wayland-im --exec-daemon --daemon-args "--xim --panel disable"
    result=$?
    # Losing the compositor during logout can make GTK exit 1 before the
    # service receives SIGTERM. Only accept that status after its session
    # has ended or begun stopping; runtime failures must still restart.
    if [ "$result" -eq 1 ]; then
      for unit in aizome.service aizome-session.target; do
        case "$(${pkgs.systemd}/bin/systemctl --user show "$unit" -p ActiveState --value)" in
          inactive|deactivating|failed) exit 0 ;;
        esac
      done
    fi
    exit "$result"
  '';
  legacy = pkgs.writeShellScript "sekka-ibus-legacy" ''
    # systemd's autostart generator can have another desktop's environment
    # when the user manager survives logout. Check the session at execution
    # too; NotShowIn alone does not prevent a second daemon in Aizome.
    ${lib.optionalString wayland ''
      [ "''${XDG_SESSION_DESKTOP:-}" != aizome ] || exit 0
    ''}
    exec ${im.package}/bin/ibus-daemon --daemonize --xim ${
      lib.optionalString (im.ibus.panel != null) "--panel=${lib.escapeShellArg im.ibus.panel}"
    }
  '';
in
{
  config = lib.mkIf (im.enable && im.type == "ibus") {
    environment.etc."xdg/autostart/ibus-daemon.desktop" = lib.mkIf (aizome || hyprland) {
      text = ''
        [Desktop Entry]
        Type=Application
        Name=IBus
        Exec=${legacy}
        NotShowIn=GNOME;KDE;${lib.optionalString hyprland "Hyprland;"}${lib.optionalString wayland "Aizome;"}
      '';
    };

    # This file is read before ~/.config/aizome/environment, so deliberate
    # user settings still win. The other desktops retain their GTK/Qt IBus
    # modules; native Aizome apps use text-input-v3 instead of those modules.
    environment.etc."xdg/aizome/environment".text = lib.mkIf wayland (
      lib.mkAfter ''
        GTK_IM_MODULE=wayland
        QT_IM_MODULE=wayland
      ''
    );

    systemd.user.services.aizome-ibus = lib.mkIf wayland {
      description = "IBus Wayland input method for Aizome";
      wantedBy = [ "aizome-session.target" ];
      partOf = [ "aizome-session.target" ];
      after = [ "aizome.service" ];
      before = [ "xdg-desktop-autostart.target" ];
      unitConfig.ConditionEnvironment = "XDG_SESSION_DESKTOP=aizome";
      # The foreground Wayland frontend owns its daemon in this service's
      # cgroup. The packaged daemon on PATH includes every configured engine.
      # Running ibus-daemon alone supplies no bridge for native Wayland apps.
      path = [ im.package ];
      serviceConfig = {
        Type = "simple";
        ExecStart = startFrontend;
        Restart = "on-failure";
        Slice = "session.slice";
      };
    };
  };
}
