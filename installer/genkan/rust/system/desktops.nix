# SPDX-License-Identifier: GPL-3.0-or-later
# Desktop sessions chosen in the installer.
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.calamares;
  # Login session name for each installer choice.
  sessions = {
    plasma = "plasma";
    gnome = "gnome";
    xfce = "xfce";
    # No longer offered by the installer (they duplicated Xfce and caused
    # most of the interference between desktops); kept so that systems
    # installed with them keep working.
    cinnamon = "cinnamon";
    mate = "mate";
    lxqt = "lxqt";
    # uwsm starts graphical-session.target, which Hyprland alone never does.
    hyprland = "hyprland-uwsm";
    tatami = "tatami";
  };
  # Former names, still accepted, so systems installed before a rename keep
  # working without edits: Tatami was called "omarchy".
  renamed = {
    omarchy = "tatami";
  };
  current = name: renamed.${name} or name;
  desktop = lib.types.enum (builtins.attrNames sessions ++ builtins.attrNames renamed);
  selected = map current cfg.desktops;
  defaultDesktop = current cfg.defaultDesktop;
  has = name: builtins.elem name selected;
  sessionEnv = pkgs.callPackage ./session-env/package.nix { };
  # GNOME locks the screen only under GDM: its unlock dialog authenticates
  # through GDM, and without GDM's D-Bus service gnome-shell has no lock
  # screen at all ("Screen Locking requires the GNOME display manager"). So
  # GDM is the login screen whenever GNOME is installed; it starts the other
  # desktops too, X11 ones with NixOS's X server arguments.
  gdm = has "gnome";
  plasmaLogin = !gdm;
  # Plasma Login Manager starts X11 sessions with the bare X server (nixpkgs
  # sets only its ServerPath), which then reads neither NixOS's xorg.conf nor
  # its module path: Xfce found no input driver, so no keyboard or mouse.
  # Start it as SDDM's module does, with NixOS's X server arguments, less a
  # fixed display (the login manager picks one per session), -terminate, and
  # -logfile /dev/null, so X keeps its log in ~/.local/share/xorg.
  xserver = config.services.xserver.displayManager;
  plasmaLoginX = pkgs.writeShellScript "plasmalogin-xserver" ''
    exec ${xserver.xserverBin} ${
      toString (
        lib.filter (
          arg: !(lib.hasPrefix ":" arg || arg == "-terminate" || lib.hasPrefix "-logfile " arg)
        ) xserver.xserverArgs
      )
    } "$@"
  '';
  # Each login through the login screen begins from the user manager's own
  # environment (see calamares-session-env below).
  sessionEnvRule = {
    # After pam_systemd, which starts the user manager.
    order = 20000;
    control = "optional";
    modulePath = "${config.security.pam.package}/lib/security/pam_exec.so";
    args = [
      "quiet"
      "type=open_session"
      (lib.getExe sessionEnv)
      "reset"
    ];
  };
  severalDesktops = lib.length selected > 1;
  # GDM's greeter lists every session it finds through the system-wide
  # XDG_DATA_DIRS, whatever the daemon's. But /etc/X11/sessions comes first,
  # for Wayland sessions too, and a Hidden entry there removes that name from
  # the list. These are the other installed sessions: NixOS's own and those that
  # packages bring (Xfce's experimental Wayland session, and the Yaru icon
  # theme's GNOME sessions, from Tatami).
  hiddenSessions = lib.subtractLists (map (desktop: sessions.${desktop}) selected) (
    lib.unique (
      config.services.displayManager.sessionData.sessionNames
      ++ [
        "xfce-wayland"
        "Yaru"
        "Yaru-xorg"
      ]
    )
  );
  x11 = [
    "xfce"
    "cinnamon"
    "mate"
    "lxqt"
  ];
  # One login entry per chosen desktop, from the sessions NixOS installs.
  loginSessions = pkgs.linkFarm "login-sessions" (
    map (
      desktop:
      let
        file = "share/${
          if lib.elem desktop x11 then "xsessions" else "wayland-sessions"
        }/${sessions.${desktop}}.desktop";
      in
      {
        name = file;
        path = "${config.services.displayManager.sessionData.desktops}/${file}";
      }
    ) selected
  );
  # Tatami's wallpapers, offered by every desktop (Tatami's module provides
  # the package even when Tatami itself is not chosen).
  wallpapers = config.programs.tatami.wallpapers;
  wallpaper = wallpapers.installed;
  # Plasma takes its default wallpaper from the Global Theme. This one is
  # Breeze with the ocean wave: Plasma falls back to Breeze for everything it
  # leaves out, as it does for Breeze Dark. Its files must be inside its own
  # directory: Plasma ignores package files that resolve elsewhere.
  breeze = "${pkgs.kdePackages.plasma-workspace}/share/plasma/look-and-feel/org.kde.breeze.desktop";
  plasmaThemeMetadata = pkgs.writeText "breeze-ocean-metadata.json" (
    builtins.toJSON {
      KPackageStructure = "Plasma/LookAndFeel";
      KPlugin = {
        Id = "org.nixos.breeze-ocean.desktop";
        Name = "Breeze Ocean";
        Description = "Breeze with the Blue ocean wave wallpaper";
        Authors = [ { Name = "KDE Visual Design Group"; } ];
        License = "GPL-2.0-or-later";
        Category = "";
      };
    }
  );
  plasmaTheme = pkgs.runCommand "breeze-ocean-look-and-feel" { } ''
    theme=$out/share/plasma/look-and-feel/org.nixos.breeze-ocean.desktop
    mkdir -p $theme/contents
    cp ${plasmaThemeMetadata} $theme/metadata.json
    substitute ${breeze}/contents/defaults $theme/contents/defaults \
      --replace-fail Image=Next Image=tatami-ocean-wave
  '';
  # Xfce's default backdrop is fixed when xfdesktop is built.
  xfdesktop = pkgs.xfdesktop.overrideAttrs (old: {
    configureFlags = old.configureFlags or [ ] ++ [
      "--with-default-backdrop-filename=${wallpaper "looks-flat.jpg"}"
    ];
  });
in
{
  # Tatami's module comes from its own repository (the flake's tatami
  # input); these keep older option names working.
  imports = [
    ./hyprland.nix
    (lib.mkRenamedOptionModule [ "calamares" "omarchy" "enable" ] [ "programs" "tatami" "enable" ])
    (lib.mkRenamedOptionModule [ "calamares" "tatami" "enable" ] [ "programs" "tatami" "enable" ])
    # GDM lists exactly the chosen desktops, one entry each (hiddenSessions).
    {
      environment.etc = lib.mkIf gdm (
        lib.listToAttrs (
          map (
            name:
            lib.nameValuePair "X11/sessions/${name}.desktop" {
              text = ''
                [Desktop Entry]
                Name=${name}
                Hidden=true
              '';
            }
          ) hiddenSessions
        )
      );
    }
  ];
  options.calamares = {
    desktops = lib.mkOption {
      type = lib.types.listOf desktop;
      default = [ "plasma" ];
      description = "Desktop sessions selected in the graphical installer.";
    };
    defaultDesktop = lib.mkOption {
      type = desktop;
      default = "plasma";
      description = "Default login session selected in the graphical installer.";
    };
  };
  config = {
    # Yukimi, for seeing and changing what is installed without editing
    # configuration, on every desktop. Its module comes from the yukimi
    # input (the flake's nixosModules.desktops, or the media's reference
    # systems).
    programs.yukimi.enable = lib.mkDefault (selected != [ ]);
    assertions = [
      {
        assertion = selected != [ ] && lib.allUnique selected;
        message = "Select one or more distinct desktops.";
      }
      {
        assertion = has defaultDesktop;
        message = "The default desktop must be one of the selected desktops.";
      }
      {
        assertion = !(has "gnome" && has "cinnamon");
        message = "GNOME and Cinnamon cannot be combined: their NixOS modules conflict on GSettings overrides.";
      }
    ];
    services.xserver.enable = true;
    services.displayManager.gdm.enable = gdm;
    # Otherwise Plasma Login Manager, NixOS's default for graphical systems: a
    # Wayland login screen (KWin) that lights whichever GPU the displays are
    # on, and that also starts X11 sessions. SDDM's X11 greeter needs a fixed
    # GPU layout and stayed black on a laptop whose panel is wired to its
    # NVIDIA GPU; SDDM's Wayland greeter races kwallet-pam (sddm/sddm#1443).
    services.displayManager.plasma-login-manager = lib.mkIf plasmaLogin {
      enable = true;
      settings.Greeter.PreselectedSession = "${sessions.${defaultDesktop}}.desktop";
      settings.X11.ServerPath = "${plasmaLoginX}";
    };
    # The login screen finds its sessions through XDG_DATA_DIRS. It lists
    # each chosen desktop once: not Plasma's X11 session, Hyprland without
    # uwsm (no graphical-session.target, so no portals or session services),
    # or Cinnamon's software-rendering and Wayland variants. Those sessions
    # stay installed.
    systemd.services.plasmalogin.environment.XDG_DATA_DIRS = lib.mkIf plasmaLogin (
      lib.mkForce "${loginSessions}/share"
    );
    systemd.user.services.plasma-login.environment.XDG_DATA_DIRS = lib.mkIf plasmaLogin (
      lib.mkForce "${loginSessions}/share"
    );
    services.displayManager.defaultSession = sessions.${defaultDesktop};
    services.desktopManager.plasma6.enable = has "plasma";
    services.desktopManager.gnome.enable = has "gnome";
    services.xserver.desktopManager.xfce.enable = has "xfce";
    services.xserver.desktopManager.cinnamon.enable = has "cinnamon";
    services.xserver.desktopManager.mate.enable = has "mate";
    services.xserver.desktopManager.lxqt.enable = has "lxqt";
    calamares.hyprland.enable = has "hyprland";
    programs.tatami.enable = has "tatami";
    # Several desktops set equally-prioritized defaults for these shared
    # helpers; resolve them explicitly. Portals remain per-session.
    programs.ssh.askPassword =
      if has "plasma" then
        "${pkgs.kdePackages.ksshaskpass}/bin/ksshaskpass"
      else
        "${pkgs.x11_ssh_askpass}/libexec/x11-ssh-askpass";
    programs.gnupg.agent.pinentryPackage =
      if has "plasma" then pkgs.pinentry-qt else pkgs.pinentry-gnome3;
    # One keyring in every desktop: GNOME Keyring, unlocked at login. Plasma
    # keeps the KWallet API for KDE applications (and Chromium and Electron
    # use it in Plasma), but with GNOME Keyring's default collection as its
    # storage instead of ksecretd, so a password saved in one desktop is there
    # in the others. Without kwallet-pam no ksecretd starts at login, which
    # had stayed behind in every other desktop's sessions.
    services.gnome.gnome-keyring.enable = true;
    environment.etc."xdg/kwalletrc" = lib.mkIf (has "plasma") {
      text = ''
        [KSecretD]
        Enabled=false
      '';
    };
    security.pam.services.login = lib.mkIf (has "plasma") { kwallet.enable = lib.mkForce false; };
    security.pam.services.kde = lib.mkIf (has "plasma") { kwallet.enable = lib.mkForce false; };
    # Xfce installs polkit-gnome system-wide, and its autostart entry has no
    # OnlyShowIn, so it also starts in Plasma, Hyprland and the others and
    # races their own authentication agents. /etc/xdg is searched first.
    environment.etc."xdg/autostart/polkit-gnome-authentication-agent-1.desktop" =
      lib.mkIf (has "xfce")
        {
          text = ''
            [Desktop Entry]
            Type=Application
            Name=PolicyKit Authentication Agent
            Exec=${pkgs.polkit_gnome}/libexec/polkit-gnome-authentication-agent-1
            NoDisplay=true
            OnlyShowIn=XFCE;
          '';
        };
    # LXQt asks for a window manager at the first login when it finds more
    # than one, and the other desktops bring KWin, Marco and Xfwm4. Openbox is
    # the one nixpkgs installs for it and what it uses when installed alone.
    # The packaged defaults still apply; a choice in LXQt's settings wins.
    environment.etc."xdg/lxqt/session.conf" = lib.mkIf (has "lxqt") {
      text = ''
        [General]
        window_manager=openbox
      '';
    };
    # LXQt's file manager writes its desktop shortcuts (Home, Trash, Computer,
    # Network) into ~/Desktop as launchers, which Plasma, Xfce, MATE and
    # Cinnamon then show as untrusted icons. It reads the first settings file
    # it finds, so this is the packaged one without the shortcuts.
    environment.etc."xdg/pcmanfm-qt/lxqt/settings.conf" =
      lib.mkIf
        (
          has "lxqt"
          && lib.any has [
            "plasma"
            "xfce"
            "mate"
            "cinnamon"
          ]
        )
        {
          source = pkgs.substitute {
            name = "pcmanfm-qt-lxqt-settings.conf";
            src = "${pkgs.lxqt.pcmanfm-qt}/share/pcmanfm-qt/lxqt/settings.conf";
            substitutions = [
              "--replace-fail"
              "DesktopShortcuts=Home, Trash, Computer, Network"
              "DesktopShortcuts="
            ];
          };
        };
    # Desktops export their environment into the user's systemd manager,
    # which outlives each session, so the next desktop's services inherited
    # it: after an X11 desktop, plasmashell ran on X11 (QT_QPA_PLATFORM=xcb)
    # without its panel. The manager records its environment as it starts,
    # and each login through the login screen begins by putting it back.
    systemd.user.services.calamares-session-env = lib.mkIf severalDesktops {
      description = "Record the environment of the user manager before any desktop session";
      wantedBy = [ "default.target" ];
      before = [ "default.target" ];
      serviceConfig = {
        Type = "oneshot";
        ExecStart = "${lib.getExe sessionEnv} save %t/calamares-session-env";
      };
    };
    security.pam.services.plasmalogin = lib.mkIf (plasmaLogin && severalDesktops) {
      rules.session.calamares-session-env = sessionEnvRule;
    };
    security.pam.services.gdm-password = lib.mkIf (gdm && severalDesktops) {
      rules.session.calamares-session-env = sessionEnvRule;
    };
    # Every desktop offers the same wallpapers from one copy of each: Plasma
    # and GNOME list them in their settings, Xfce in its backdrop folders and
    # Tatami in Style › Background. Each desktop starts with its own: the
    # ocean wave in Plasma, the foggy forest in GNOME, the mountains at dusk
    # in Xfce and Da Nang at night in Tatami.
    environment.systemPackages = [
      wallpapers
    ]
    ++ lib.optional (has "plasma") plasmaTheme
    ++ lib.optional (has "xfce") xfdesktop;
    environment.pathsToLink = [
      "/share/backgrounds"
      "/share/wallpapers"
      "/share/gnome-background-properties"
    ];
    environment.etc."xdg/kdeglobals" = lib.mkIf (has "plasma") {
      text = ''
        [KDE]
        LookAndFeelPackage=org.nixos.breeze-ocean.desktop
      '';
    };
    services.desktopManager.gnome.extraGSettingsOverrides = lib.mkIf (has "gnome") ''
      [org.gnome.desktop.background]
      picture-uri='file://${wallpaper "foggy-forest.jpg"}'
      picture-uri-dark='file://${wallpaper "foggy-forest.jpg"}'

      [org.gnome.desktop.screensaver]
      picture-uri='file://${wallpaper "foggy-forest.jpg"}'
    '';
    environment.xfce.excludePackages = lib.mkIf (has "xfce") [ pkgs.xfdesktop ];
    # GNOME enables IBus, whose autostart entry skips only GNOME and KDE. In
    # the Hyprland sessions it only reports that it should run from GNOME.
    environment.etc."xdg/autostart/ibus-daemon.desktop" =
      lib.mkIf
        (
          (has "hyprland" || has "tatami")
          && config.i18n.inputMethod.enable
          && config.i18n.inputMethod.type == "ibus"
        )
        {
          text = ''
            [Desktop Entry]
            Type=Application
            Name=IBus
            Exec=${config.i18n.inputMethod.package}/bin/ibus-daemon --daemonize --xim
            NotShowIn=GNOME;KDE;Hyprland;
          '';
        };
  };
}
