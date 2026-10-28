# Aizome as a NixOS module (this flake's nixosModules.default):
# `programs.aizome.enable = true;` adds the Aizome session to the login
# screen, with what its shell and apps stand on: the portals, the services
# behind Settings (NetworkManager, BlueZ, UPower, power profiles, UDisks2,
# AccountsService, CUPS, geoclue), the lock screen's PAM service, the fonts,
# and foot themed for Aizome. Everything session-specific is read only in
# Aizome sessions (/etc/xdg/aizome, the `aizome` dconf profile), so other
# desktops on the same machine keep their own look.
{ self }:
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.programs.aizome;
  system = pkgs.stdenv.hostPlatform.system;
  archivo = pkgs.callPackage ./fonts.nix { };
  # The login entry runs the session from the system profile rather than a
  # store path, so an update switches without logging out and the next
  # login starts the new Aizome (as Sekka's Tatami entry does).
  session = pkgs.writeTextFile {
    name = "aizome-session-entry";
    destination = "/share/wayland-sessions/aizome.desktop";
    text = ''
      [Desktop Entry]
      Name=Aizome
      Comment=Sekka's desktop: Haiku tabs, NeXT menus and IRIX's Toolchest, in indigo
      Exec=/run/current-system/sw/bin/aizome-session
      Type=Application
      DesktopNames=Aizome
    '';
    derivationArgs.passthru.providedSessions = [ "aizome" ];
  };
  # Screen sharing: xdg-desktop-portal-wlr asks Aizome's picker what to
  # share (aizome-share-picker: the compositor's picker, a display or a
  # window, with a banner saying why). The portal reads its config only
  # from ~/.config or its own package, not XDG_CONFIG_DIRS, so in Aizome
  # sessions it's started with /etc/xdg/aizome's, unless the person has
  # configured it themselves; other desktops start it as before.
  sharePickerConfig = ''
    [screencast]
    chooser_type=dmenu
    chooser_cmd=${cfg.package}/bin/aizome-share-picker
  '';
  # The portal's captures, patched: one in flight when the stream pauses
  # (as it does once while a consumer links) is cancelled and its buffer
  # kept, rather than queued unfilled with a timestamp of zero, after which
  # a consumer that syncs to the timestamps stalls; a frame's late events no
  # longer land on the capture after it; and closing a stalled share no
  # longer frees the retry timer twice.
  xdg-desktop-portal-wlr = pkgs.xdg-desktop-portal-wlr.overrideAttrs (old: {
    patches = (old.patches or [ ]) ++ [ ./xdg-desktop-portal-wlr-captures.patch ];
  });
  # Another desktop on the system (sway, say) may enable NixOS's own
  # xdg.portal.wlr, which installs nixpkgs' portal and sets its ExecStart.
  # Then that package provides the unit and the portal files (installing
  # Aizome's as well would collide), and Aizome's ExecStart, set through
  # the same option, still runs the patched portal: in Aizome with
  # Aizome's picker, elsewhere with the configuration NixOS writes.
  wlr = config.xdg.portal.wlr;
  wlrConfig = (pkgs.formats.ini { }).generate "xdg-desktop-portal-wlr.ini" wlr.settings;
  portalWlr = pkgs.writeShellScript "xdg-desktop-portal-wlr-aizome" ''
    own=''${XDG_CONFIG_HOME:-$HOME/.config}/xdg-desktop-portal-wlr
    case ":''${XDG_CURRENT_DESKTOP:-}:" in
      *:Aizome:*)
        if [ ! -e "$own/Aizome" ] && [ ! -e "$own/config" ]; then
          set -- -c /etc/xdg/aizome/xdg-desktop-portal-wlr/config "$@"
        fi
        ;;
      ${lib.optionalString wlr.enable ''
        *)
          set -- --config=${wlrConfig} "$@"
          ;;
      ''}
    esac
    exec ${xdg-desktop-portal-wlr}/libexec/xdg-desktop-portal-wlr "$@"
  '';
  # Other installed desktops keep their network applet; Aizome has network
  # controls and a credential agent of its own. This condition is what keeps
  # the applet out of Aizome, for programs.nm-applet's service and the unit
  # systemd generates from its XDG autostart entry alike: the generator
  # reads only the user manager's own environment, not the session's
  # imported variables, so an autostart override for Aizome alone can't
  # work. aizome-session always sets XDG_SESSION_DESKTOP, which, unlike
  # XDG_CURRENT_DESKTOP, ~/.config/aizome/environment can't change.
  otherDesktop = pkgs.writeShellScript "nm-applet-outside-aizome" ''
    if [ "''${XDG_SESSION_DESKTOP:-}" = aizome ]; then
      echo "nm-applet is not started in Aizome, whose network agent asks for credentials" >&2
      exit 1
    fi
  '';
  nmAppletDropIns = pkgs.runCommand "aizome-nm-applet-session-policy" { } ''
    for unit in 'nm-applet.service' 'app-nm\x2dapplet@autostart.service'; do
      mkdir -p "$out/lib/systemd/user/$unit.d"
      cat > "$out/lib/systemd/user/$unit.d/aizome.conf" <<EOF
    [Service]
    ExecCondition=${otherDesktop}
    EOF
    done
  '';
  # wf-recorder (the fallback recorder) puts its wl_shm buffers in files in
  # /tmp, which on most NixOS systems is on disk: every frame the compositor
  # copies would be written out (over 1 GB/s at 2880 × 1800 and 60 fps) and
  # the compositor would wait on the disk. In memfds they stay in memory.
  wf-recorder = pkgs.wf-recorder.overrideAttrs (old: {
    postPatch = (old.postPatch or "") + ''
      substituteInPlace src/main.cpp \
        --replace-fail 'int fd = mkstemp(name);' 'int fd = memfd_create("wf-recorder-shared", MFD_CLOEXEC);'
    '';
  });
  # Read by aizome-session before it starts the compositor: what every
  # program in the session sees. SDL_VIDEODRIVER is left unset: older SDL 2
  # builds, which many games bundle, read "wayland,x11" as one driver name
  # and fail to start, and current SDL picks a working driver on its own.
  environment = {
    XDG_CURRENT_DESKTOP = "Aizome";
    XDG_SESSION_DESKTOP = "aizome";
    XDG_SESSION_TYPE = "wayland";
    XDG_CONFIG_DIRS = "/etc/xdg/aizome:/etc/xdg";
    DCONF_PROFILE = "aizome";
    NIXOS_OZONE_WL = "1";
    MOZ_ENABLE_WAYLAND = "1";
    QT_QPA_PLATFORM = "wayland;xcb";
    QT_QPA_PLATFORMTHEME = "gtk3";
    # GTK 3 apps and Firefox open and save through the portal, so in
    # Aizome's own chooser (Chromium, Electron apps and sandboxed apps
    # always ask the portal).
    GTK_USE_PORTAL = "1";
    _JAVA_AWT_WM_NONREPARENTING = "1";
    AIZOME_WALLPAPER_NIGHT = toString cfg.wallpapers.night;
    AIZOME_WALLPAPER_DAY = toString cfg.wallpapers.day;
  };
in
{
  # The login screen, aizome-greeter on greetd (programs.aizome.greeter).
  imports = [ ./greeter.nix ];

  options.programs.aizome = {
    enable = lib.mkEnableOption "the Aizome desktop session";
    package = lib.mkOption {
      type = lib.types.package;
      default = self.packages.${system}.aizome;
      defaultText = lib.literalExpression "aizome.packages.\${system}.aizome";
      description = "The Aizome package: compositor, shell, aizomectl and apps.";
    };
    mutableTimeZone = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = ''
        Let Settings (and the automatic time zone) change the time zone.
        Aizome takes `time.timeZone` over, starting from the zone the
        system was last set to, so changes made in Settings last.
      '';
    };
    videoAcceleration = lib.mkOption {
      type = lib.types.bool;
      default = pkgs.stdenv.hostPlatform.isx86_64;
      defaultText = lib.literalExpression "pkgs.stdenv.hostPlatform.isx86_64";
      description = ''
        Install Intel's VA-API driver (intel-media-driver), so screen
        recordings encode on Intel GPUs (and video players decode there)
        rather than on the CPU. AMD's driver comes with Mesa, NVIDIA's
        encoder with its driver.
      '';
    };
    alarmsWakeSystem = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Let Tokei's alarms wake the computer from sleep (CAP_WAKE_ALARM for user services).";
    };
    wallpapers = {
      night = lib.mkOption {
        type = lib.types.nullOr lib.types.path;
        default = null;
        example = lib.literalExpression ''"''${tatami-wallpapers}/share/backgrounds/tatami/da-nang-at-night.jpg"'';
        description = "The Night theme's default wallpaper. Without one the desktop is kachi, the near-black indigo.";
      };
      day = lib.mkOption {
        type = lib.types.nullOr lib.types.path;
        default = null;
        description = "The Day theme's default wallpaper.";
      };
    };
  };

  config = lib.mkIf cfg.enable {
    environment.systemPackages = [
      cfg.package
      pkgs.foot
      pkgs.xwayland-satellite
      pkgs.wl-clipboard
      pkgs.xdg-utils
      pkgs.adwaita-icon-theme
      pkgs.hicolor-icon-theme
      pkgs.shared-mime-info
      # GPU names for Activity and About (aizome-sys's sysinfo).
      pkgs.hwdata
      # Keyboard layouts and options for Settings (evdev.xml), linked
      # into /run/current-system/sw/share/X11/xkb.
      pkgs.xkeyboard_config
      # Hako compresses and extracts archives with bsdtar.
      pkgs.libarchive
      # Tokei rings with pw-play and the freedesktop sound theme.
      pkgs.pipewire
      pkgs.sound-theme-freedesktop
      pkgs.libnotify
      # Screen recording where gpu-screen-recorder can't run (no hardware
      # OpenGL, as in a VM): through the compositor's wlr-screencopy. Ahead
      # of another desktop's (Tatami installs it too).
      (lib.hiPrio wf-recorder)
      # vainfo: whether wf-recorder can encode on the GPU (VA-API), which
      # the shell asks once a session rather than finding out mid-recording.
      pkgs.libva-utils
    ];
    # Screen recording (the palette's Record Screen and Record Area, ⌥Print):
    # gpu-screen-recorder, with the capability wrapper its KMS capture needs
    # (gsr-kms-server, CAP_SYS_ADMIN), as Tatami has it.
    programs.gpu-screen-recorder.enable = lib.mkDefault true;
    # The GPU's drivers (OpenGL for the compositor, VA-API for recordings)
    # in /run/opengl-driver.
    hardware.graphics.enable = lib.mkDefault true;
    hardware.graphics.extraPackages = lib.mkIf cfg.videoAcceleration [ pkgs.intel-media-driver ];
    services.displayManager.sessionPackages = [ session ];
    systemd.packages = [
      cfg.package
      nmAppletDropIns
    ];
    systemd.user.services.xdg-desktop-portal-wlr = {
      overrideStrategy = "asDropin";
      serviceConfig.ExecStart = lib.mkForce [
        ""
        "${portalWlr}"
      ];
    };

    # Under load, the session (the compositor, which also weighs 1000 within
    # it, pipewire, the portals) runs before apps, so the desktop keeps
    # drawing and answering and Ctrl+Alt+Del opens at once. Raising
    # session.slice rather than lowering app.slice keeps apps ahead of
    # background.slice (30): 400 : 100 : 30.
    systemd.user.slices.session.sliceConfig = {
      CPUWeight = lib.mkDefault 400;
      IOWeight = lib.mkDefault 400;
    };

    # In Aizome, Settings owns the time zone and the computer's name:
    # Sekka lets its desktop configure more than a desktop usually does.
    # The time zone starts from the one the system is set to now (NixOS's
    # link while time.timeZone was set), saved before /etc is rebuilt and
    # put back if the rebuild removed it; after that timedated keeps it.
    time.timeZone = lib.mkIf cfg.mutableTimeZone (lib.mkOverride 90 null);
    # timedated and NixOS can both write absolute /etc/zoneinfo links.
    # Tell Settings the effective policy instead of guessing from that link.
    systemd.services.systemd-timedated.environment.AIZOME_TIMEZONE_MANAGED =
      if config.time.timeZone == null then "0" else "1";
    system.activationScripts = lib.mkIf cfg.mutableTimeZone {
      aizomeTimeZoneSave.text = ''
        if zone=$(readlink -f /etc/localtime 2>/dev/null) && [ -f "$zone" ]; then
          case "$zone" in
            */zoneinfo/*)
              mkdir -p /var/lib/aizome
              printf '%s\n' "''${zone#*/zoneinfo/}" > /var/lib/aizome/timezone
              ;;
          esac
        fi
      '';
      etc.deps = [ "aizomeTimeZoneSave" ];
      # Preserve the selected zone through activation, including a link
      # left by an earlier configuration that pinned time.timeZone.
      # Settings uses the explicit policy above, not the link's spelling.
      aizomeTimeZoneRestore = {
        deps = [ "etc" ];
        text = ''
          zone=
          if target=$(readlink /etc/localtime 2>/dev/null); then
            case "$target" in
              /etc/zoneinfo/*) zone=''${target#/etc/zoneinfo/} ;;
            esac
          elif [ ! -e /etc/localtime ] && [ -s /var/lib/aizome/timezone ]; then
            zone=$(cat /var/lib/aizome/timezone)
          fi
          if [ -n "$zone" ] && [ -e "/etc/zoneinfo/$zone" ]; then
            ln -sfn "../etc/zoneinfo/$zone" /etc/localtime
          fi
        '';
      };
    };
    # The pretty name ("The studio laptop") in a file hostnamed can write;
    # the host name itself stays networking.hostName.
    systemd.services.systemd-hostnamed.environment.SYSTEMD_ETC_MACHINE_INFO =
      lib.mkForce "/etc/machine-info";

    # Tokei's alarms are transient systemd timers, which end with the user
    # manager: schedule them again at each login, in any desktop.
    systemd.user.services.aizome-tokei-alarms = {
      description = "Schedule Tokei's alarms";
      wantedBy = [ "graphical-session.target" ];
      partOf = [ "graphical-session.target" ];
      serviceConfig = {
        Type = "oneshot";
        ExecStart = "${cfg.package}/bin/aizome-tokei --sync";
      };
    };
    # Koyomi's alerts, as notifications, whether or not Koyomi is open
    # (the unit comes with the package).
    systemd.user.services.aizome-koyomi-agent.wantedBy = [ "graphical-session.target" ];
    # Koe's dictation starts with the session for those who turned it on
    # in Koe (the unit comes with the package; `aizome-koe enabled` is its
    # condition). Nobody else runs it, and no model is downloaded until
    # someone asks. It records with PipeWire's pw-record and copies with
    # wl-copy, which NixOS doesn't give units it configures.
    systemd.user.services.aizome-koe = {
      wantedBy = [ "aizome-session.target" ];
      path = [
        pkgs.pipewire
        pkgs.wl-clipboard
      ];
    };
    # After `nixos-rebuild switch`, a notification offers to reload Aizome
    # (its window manager and shell) without closing anything; a changed
    # compositor core waits for the next login (docs/reload.md).
    systemd.user.paths.aizome-update.wantedBy = [ "aizome-session.target" ];
    # Keep Awake starts with the session when it was left on (the unit
    # comes with the package; docs/keep-awake.md). A system update doesn't
    # restart it, so its locks are never let go in between.
    systemd.user.services.aizome-keep-awake = {
      wantedBy = [ "aizome-session.target" ];
      restartIfChanged = false;
    };
    # An alarm can wake the computer (Tokei's "Wake the computer"), which
    # needs CAP_WAKE_ALARM for the user's timers.
    systemd.services."user@".serviceConfig.AmbientCapabilities = lib.mkIf cfg.alarmsWakeSystem [
      "CAP_WAKE_ALARM"
    ];

    environment.etc = {
      "xdg/aizome/environment".text = lib.concatStrings (
        lib.mapAttrsToList (name: value: "${name}=${value}\n") (
          lib.filterAttrs (_: value: value != "") environment
        )
      );
      "xdg/aizome/foot/foot.ini".source = ../etc/foot/foot.ini;
      "xdg/aizome/xdg-desktop-portal-wlr/config".text = sharePickerConfig;
      # Aizome's apps open what they're for in Aizome sessions: the
      # desktop-specific list (XDG_CURRENT_DESKTOP=Aizome) is read before
      # mimeapps.list, so other desktops keep their own defaults, and the
      # user's own choices (~/.config/mimeapps.list) still win.
      "xdg/aizome-mimeapps.list".text =
        let
          entry = app: types: lib.concatMapStrings (type: "${type}=org.aizome.${app}.desktop;\n") types;
        in
        ''
          [Default Applications]
        ''
        + entry "Shashin" [
          "image/png"
          "image/apng"
          "image/jpeg"
          "image/gif"
          "image/webp"
          "image/bmp"
          "image/tiff"
          "image/x-icon"
          "image/vnd.microsoft.icon"
          "image/svg+xml"
        ]
        + entry "Sumi" [
          "text/plain"
          "text/markdown"
          "text/x-markdown"
          "text/rust"
          "text/x-python"
          "text/x-nix"
          "application/x-shellscript"
          "application/toml"
          "application/json"
          "application/x-yaml"
        ]
        + entry "Utsushi" [
          "video/mp4"
          "video/x-matroska"
          "video/webm"
          "video/quicktime"
          "video/x-msvideo"
          "video/vnd.avi"
          "video/mpeg"
          "video/ogg"
          "video/3gpp"
          "video/mp2t"
          "video/x-flv"
          "video/x-m4v"
          "video/av1"
          "audio/mpeg"
          "audio/mp4"
          "audio/x-m4a"
          "audio/aac"
          "audio/flac"
          "audio/x-flac"
          "audio/ogg"
          "audio/x-vorbis+ogg"
          "audio/opus"
          "audio/x-opus+ogg"
          "audio/wav"
          "audio/x-wav"
          "audio/webm"
          "audio/x-matroska"
        ]
        + entry "Hako" [ "inode/directory" ]
        + entry "Koyomi" [ "text/calendar" ]
        + entry "Tansu" [
          "application/x-cd-image"
          "application/x-raw-disk-image"
        ];
      # The welcome tour, once, at the first Aizome login (it decides for
      # itself whether to show; OnlyShowIn=Aizome keeps it out of others).
      "xdg/autostart/org.aizome.Annai.Welcome.desktop".source =
        "${cfg.package}/share/applications/org.aizome.Annai.Welcome.desktop";
    };

    # File choosers through Aizome's own backend (aizome-portal, which D-Bus
    # starts when one is asked for); screenshots and screencasts
    # (browsers' screen and window sharing) through the compositor's
    # ext-image-copy-capture; settings and the rest through GTK's portal.
    xdg.portal = {
      enable = true;
      extraPortals = lib.optional (!wlr.enable) xdg-desktop-portal-wlr ++ [
        pkgs.xdg-desktop-portal-gtk
        cfg.package
      ];
      configPackages = [ cfg.package ];
    };
    # Terminal applications open in foot in Aizome sessions.
    xdg.terminal-exec = {
      enable = true;
      settings.Aizome = [ "foot.desktop" ];
    };

    # The lock screen authenticates through PAM as this service.
    security.pam.services.aizome = { };
    security.polkit.enable = true;
    # Someone at the computer who administers it (wheel) changes from
    # Settings what a Mac's owner changes there without a password prompt,
    # as GNOME lets them: the time and time zone, the computer's name, the
    # language and keyboard, and networks (joining, airplane mode, sharing),
    # which NixOS otherwise leaves to the networkmanager group. Others are
    # asked as systemd and NetworkManager ask.
    security.polkit.extraConfig = ''
      polkit.addRule(function (action, subject) {
        var settings = [
          "org.freedesktop.timedate1.set-time",
          "org.freedesktop.timedate1.set-timezone",
          "org.freedesktop.timedate1.set-local-rtc",
          "org.freedesktop.timedate1.set-ntp",
          "org.freedesktop.hostname1.set-hostname",
          "org.freedesktop.hostname1.set-static-hostname",
          "org.freedesktop.hostname1.set-machine-info",
          "org.freedesktop.locale1.set-locale",
          "org.freedesktop.locale1.set-keyboard",
        ];
        if (subject.local && subject.active && subject.isInGroup("wheel") &&
            (settings.indexOf(action.id) >= 0 ||
             action.id.indexOf("org.freedesktop.NetworkManager.") == 0)) {
          return polkit.Result.YES;
        }
      });
    '';

    # What Settings, the Deskbar's widgets and the apps talk to.
    networking.networkmanager.enable = lib.mkDefault true;
    hardware.bluetooth.enable = lib.mkDefault true;
    services.upower.enable = lib.mkDefault true;
    services.power-profiles-daemon.enable = lib.mkDefault true;
    services.udisks2.enable = lib.mkDefault true;
    services.accounts-daemon.enable = lib.mkDefault true;
    services.printing.enable = lib.mkDefault true;
    # Network and driverless (IPP Everywhere) printers announce themselves
    # over mDNS, which is how Settings finds them.
    services.avahi = {
      enable = lib.mkDefault true;
      nssmdns4 = lib.mkDefault true;
    };
    services.geoclue2.enable = lib.mkDefault true;
    # Tenki's "Current Location", and Settings' automatic time zone. They
    # ask for city accuracy; the location comes from nixpkgs' default
    # provider (BeaconDB).
    services.geoclue2.appConfig = {
      "org.aizome.Tenki" = {
        isAllowed = true;
        isSystem = false;
      };
      "org.aizome.Settei" = {
        isAllowed = true;
        isSystem = false;
      };
      # The shell: where the computer is, for Night Light's sunset
      # schedule and Auto's theme (it falls back to the time zone's city).
      "org.aizome.Shell" = {
        isAllowed = true;
        isSystem = false;
      };
    };
    services.gvfs.enable = lib.mkDefault true;
    services.gnome.gnome-keyring.enable = lib.mkDefault true;
    services.pipewire = {
      enable = lib.mkDefault true;
      pulse.enable = lib.mkDefault true;
      wireplumber.enable = lib.mkDefault true;
      # Streams that ask for long graph cycles get them: Utsushi's music
      # asks for 8192 frames (170 ms), so with nothing else playing the
      # sound card, PipeWire and the player wake about six times a second
      # rather than twenty-three at PipeWire's usual cap of 2048. Streams
      # asking for short cycles (calls, games) still get theirs, and the
      # usual cycle (1024) is unchanged for those that don't ask.
      extraConfig.pipewire."90-aizome-long-cycles"."context.properties"."default.clock.max-quantum" =
        lib.mkDefault 8192;
    };
    security.rtkit.enable = lib.mkDefault true;

    # GTK and Qt applications follow Aizome's Night in its sessions: the
    # `aizome` dconf profile (selected by DCONF_PROFILE above) holds the
    # defaults; the user's own settings still win.
    programs.dconf.enable = true;
    programs.dconf.profiles.aizome.databases = [
      {
        settings."org/gnome/desktop/interface" = {
          color-scheme = "prefer-dark";
          gtk-theme = "Adwaita-dark";
          icon-theme = "Adwaita";
          cursor-theme = "Adwaita";
          cursor-size = lib.gvariant.mkInt32 24;
          font-name = "Archivo 11";
          document-font-name = "Archivo 11";
          monospace-font-name = "IBM Plex Mono 11";
          clock-format = "24h";
        };
        settings."org/gnome/desktop/wm/preferences".button-layout = "";
      }
    ];

    fonts.packages = [
      archivo
      pkgs.ibm-plex
      pkgs.noto-fonts
      pkgs.noto-fonts-cjk-sans
      pkgs.noto-fonts-cjk-serif
      pkgs.noto-fonts-color-emoji
    ];
  };
}
