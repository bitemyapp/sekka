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
  autoLogin = config.services.displayManager.autoLogin;
  # Login session name for each installer choice.
  sessions = {
    aizome = "aizome";
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
  # The login screen. Aizome's own (aizome-greeter on greetd) wherever
  # Aizome is chosen, listing every chosen desktop. But GNOME locks the
  # screen only under GDM: its unlock dialog authenticates through GDM, and
  # without GDM's D-Bus service gnome-shell has no lock screen at all
  # ("Screen Locking requires the GNOME display manager"). So GDM is the
  # login screen whenever GNOME is installed, unless calamares.loginScreen
  # says otherwise; it starts the other desktops too, X11 ones with NixOS's
  # X server arguments. Elsewhere, Plasma Login Manager.
  loginScreen =
    if cfg.loginScreen != "auto" then
      cfg.loginScreen
    else if has "gnome" then
      "gdm"
    else if has "aizome" then
      "aizome"
    else
      "plasma-login";
  gdm = loginScreen == "gdm";
  aizomeGreeter = loginScreen == "aizome";
  plasmaLogin = loginScreen == "plasma-login";
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
  # Keep a parent around the NixOS wrapper: a portal's Requisite can keep
  # graphical-session.target needed after the legacy X11 session stops its
  # fake target. End both before Plasma Login tears down the X server.
  plasmaLoginSession = pkgs.writeShellScript "plasmalogin-x11-session" ''
    cleanup() {
      ${config.systemd.package}/bin/systemctl --user stop nixos-fake-graphical-session.target graphical-session.target
      ${config.systemd.package}/bin/systemctl --user unset-environment DISPLAY XAUTHORITY XDG_SESSION_ID XDG_CURRENT_DESKTOP XDG_SESSION_DESKTOP XDG_SESSION_TYPE
    }
    trap cleanup EXIT
    ${config.services.displayManager.sessionData.wrapper} "$@"
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
  # The image plugin's FillMode=2 (PreserveAspectCrop) scales the original
  # proportionally and crops to each display; do not pre-crop the source.
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
  # Xfce's default backdrop is fixed when xfdesktop is built. Its default
  # XFCE_BACKDROP_IMAGE_ZOOMED mode preserves aspect ratio and crops at
  # display time, including on newly attached monitors.
  xfdesktop = pkgs.xfdesktop.overrideAttrs (old: {
    configureFlags = old.configureFlags or [ ] ++ [
      "--with-default-backdrop-filename=${wallpaper "looks-flat.jpg"}"
    ];
  });
  automatic = autoLogin.enable && autoLogin.user != null;
  # Whether the account that logs in by itself can open a session with
  # nothing asked: PAM's account check (pam_unix's check_shadow_expiry), as
  # yukimi-helper applies it when it writes the choice
  # (yukimi_system::people::blocked). It fails for an expired account, a
  # password to set at the next login, and a password as old as the account
  # allows. A field that isn't a number of days (empty, or -1) is unset,
  # and no last change means the password doesn't age. An account that is
  # gone or has no entry passes: each login screen shows itself for that.
  canLogInByItself = pkgs.writeShellScript "can-log-in-by-itself" ''
    today=$(( $(${pkgs.coreutils}/bin/date +%s) / 86400 ))
    ${pkgs.gawk}/bin/awk -F: -v user=${lib.escapeShellArg autoLogin.user} -v today="$today" '
      function days(field) { return field ~ /^[0-9]+$/ ? field + 0 : -1 }
      $1 == user {
        expires = days($8); changed = days($3); most = days($5)
        if (expires >= 0 && today >= expires) exit 1
        if (changed == 0) exit 1
        if (changed > 0 && today >= changed && most >= 0 && today - changed >= most) exit 1
        exit 0
      }' /etc/shadow
  '';
  # Plasma Login's settings for this start alone: the target of its last
  # drop-in (/etc/plasmalogin.conf.d), which only the unit below writes.
  plasmaLoginThisStart = "/run/plasmalogin-this-start.conf";
in
{
  # Tatami's module comes from its own repository (the flake's tatami
  # input); these keep older option names working.
  imports = [
    ./hyprland.nix
    ../../../../modules/portal-fixes.nix
    ../../../../modules/avahi-fixes.nix
    ../../../../modules/pipewire-fixes.nix
    ../../../../modules/gnome-keyring-fixes.nix
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
      default = [ "aizome" ];
      description = "Desktop sessions selected in the graphical installer.";
    };
    defaultDesktop = lib.mkOption {
      type = desktop;
      default = "aizome";
      description = "Default login session selected in the graphical installer.";
    };
    loginScreen = lib.mkOption {
      type = lib.types.enum [
        "auto"
        "aizome"
        "gdm"
        "plasma-login"
      ];
      default = "auto";
      description = ''
        The login screen. `auto`: GDM when GNOME is chosen (GNOME locks its
        screen only under GDM), else Aizome's own when Aizome is chosen,
        else Plasma Login Manager. `aizome` with GNOME chosen too leaves
        GNOME sessions without a lock screen.
      '';
    };
    autoLogin = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      example = "alice";
      description = ''
        The account the computer logs in to by itself at boot, without a
        password, on whichever login screen; empty for none. Settei's Users
        & Groups sets it through Yukimi, which takes anyone with an account.
        Null preserves services.displayManager.autoLogin. An explicit choice
        takes precedence over ordinary definitions of that setting; forced
        system policy still takes precedence over this choice.
      '';
    };
  };
  config = {
    # Yukimi, for seeing and changing what is installed without editing
    # configuration, on every desktop. Its module comes from the yukimi
    # input (the flake's nixosModules.desktops, or the media's reference
    # systems).
    programs.yukimi.enable = lib.mkDefault (selected != [ ]);
    # What Yukimi may change of these after installing: the desktops, the
    # default one and the login screen, to the values the installer offers.
    programs.yukimi.choices =
      let
        titles = {
          aizome = "Aizome";
          plasma = "Plasma";
          gnome = "GNOME";
          xfce = "Xfce";
          hyprland = "Hyprland";
          tatami = "Tatami";
          cinnamon = "Cinnamon";
          mate = "MATE";
          lxqt = "LXQt";
        };
        offered = [
          "aizome"
          "tatami"
          "plasma"
          "gnome"
          "xfce"
          "hyprland"
        ];
        # Those no longer offered stay choosable where they are installed.
        desktops = offered ++ lib.filter (d: !lib.elem d offered) selected;
        option = id: {
          inherit id;
          title = titles.${id};
        };
      in
      [
        {
          setting = "calamares.desktops";
          title = "Desktops";
          many = true;
          value = selected;
          options = map option desktops;
        }
        {
          setting = "calamares.defaultDesktop";
          title = "Default desktop";
          value = defaultDesktop;
          within = "calamares.desktops";
          options = map option desktops;
        }
        {
          setting = "calamares.loginScreen";
          title = "Login screen";
          value = cfg.loginScreen;
          options = [
            {
              id = "auto";
              title = "Automatic";
              detail = "GDM when GNOME is installed (GNOME locks its screen only under GDM), else Aizome's own when Aizome is, else Plasma Login.";
            }
          ]
          ++ [
            {
              id = "aizome";
              title = "Aizome";
              needs = "calamares.desktops=aizome";
              detail = "Aizome's own, listing every desktop. GNOME sessions have no lock screen under it.";
            }
            {
              id = "gdm";
              title = "GDM";
              detail = "GNOME's.";
            }
            {
              id = "plasma-login";
              title = "Plasma Login";
              detail = "KDE's.";
            }
          ];
        }
        # Anyone with an account, or no one (Settei's Users & Groups
        # proposes it): Yukimi checks the account exists as it writes it.
        {
          setting = "calamares.autoLogin";
          title = "Log in automatically";
          people = true;
          value = if autoLogin.enable && autoLogin.user != null then autoLogin.user else "";
          options = [ ];
        }
      ];
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
      {
        assertion = !aizomeGreeter || has "aizome";
        message = "Aizome's login screen (calamares.loginScreen = \"aizome\") needs Aizome among calamares.desktops.";
      }
    ];
    warnings = lib.optional (aizomeGreeter && has "gnome") ''
      calamares.loginScreen = "aizome" with GNOME chosen: GNOME locks its screen only under GDM, so GNOME sessions have no lock screen.
    '';
    services.xserver.enable = true;
    services.displayManager.gdm.enable = gdm;
    # GDM shows no login screen when the account it logs in by itself can't
    # open a session with nothing asked: one that has expired, or has a
    # password to set or change at its next login (as a person added in
    # Settei without one has). Its automatic login fails and nothing
    # follows. So before GDM starts, such an account turns automatic login
    # off for this start, in GDM's runtime configuration
    # (/run/gdm/custom.conf, gone at the next). An account that is gone needs
    # nothing: GDM shows its login screen for that itself. Aizome's login
    # screen needs none of this: greetd is started again (its module).
    systemd.services.display-manager.preStart = lib.mkIf (gdm && automatic) (
      lib.mkAfter ''
        if ! ${canLogInByItself}; then
          # Never in GDM's way: at worst it starts as it would have.
          ${pkgs.coreutils}/bin/mkdir -p /run/gdm || true
          ${pkgs.gdm}/libexec/gdm-runtime-config set daemon AutomaticLoginEnable false || true
        fi
      ''
    );
    # Otherwise Plasma Login Manager, NixOS's default for graphical systems: a
    # Wayland login screen (KWin) that lights whichever GPU the displays are
    # on, and that also starts X11 sessions. SDDM's X11 greeter needs a fixed
    # GPU layout and stayed black on a laptop whose panel is wired to its
    # NVIDIA GPU; SDDM's Wayland greeter races kwallet-pam (sddm/sddm#1443).
    services.displayManager.plasma-login-manager = lib.mkIf plasmaLogin {
      enable = true;
      # Plasma Login's built-in Xsession bypasses NixOS's session wrapper,
      # leaving graphical-session.target inactive in Xfce. Its X11 wrapper
      # is a build-time setting; keep the Wayland startup path unchanged.
      package = lib.mkDefault (
        pkgs.kdePackages.plasma-login-manager.overrideAttrs (old: {
          patches = (old.patches or [ ]) ++ [
            ../../../../nix/patches/plasma-login-config-order.patch
          ];
          cmakeFlags = (old.cmakeFlags or [ ]) ++ [
            "-DSESSION_COMMAND=${plasmaLoginSession}"
          ];
        })
      );
      settings.Greeter.PreselectedSession = "${sessions.${defaultDesktop}}.desktop";
      settings.X11.ServerPath = "${plasmaLoginX}";
    };
    # The login screen finds its sessions through XDG_DATA_DIRS. It lists
    # each chosen desktop once: not Plasma's X11 session, Hyprland without
    # uwsm (no graphical-session.target, so no portals or session services),
    # or Cinnamon's software-rendering and Wayland variants. Those sessions
    # stay installed. Under GDM there is no Plasma Login unit to change: a
    # condition on the value alone would still define an empty one, whose
    # PATH changes with coreutils and the like, and Yukimi would wait for a
    # restart to apply them.
    #
    # Plasma Login shows its login screen by itself when the account it logs
    # in is gone or has expired, or has a password to set where there is
    # none yet. Where there is a password that has to be changed (chage -d 0,
    # or one as old as the account allows), its automatic login waits for
    # the old password, which nobody is asked for, and no login screen
    # follows. So such an account turns automatic login off for this start
    # here too. Plasma Login reads no settings from /run, so the last of its
    # drop-ins (the later one wins) is a link to a file there, which only
    # this writes and which is gone at the next start.
    systemd.services.plasmalogin = lib.mkIf plasmaLogin {
      environment.XDG_DATA_DIRS = lib.mkForce "${loginSessions}/share";
      preStart = lib.mkIf automatic ''
        if ! ${canLogInByItself}; then
          # Never in Plasma Login's way: at worst it starts as it would have.
          ${pkgs.coreutils}/bin/printf '[Autologin]\nUser=\n' > ${plasmaLoginThisStart} || true
        fi
      '';
    };
    environment.etc."plasmalogin.conf.d/zz-this-start.conf" = lib.mkIf (plasmaLogin && automatic) {
      source = plasmaLoginThisStart;
    };
    systemd.user.services.plasma-login = lib.mkIf plasmaLogin {
      environment.XDG_DATA_DIRS = lib.mkForce "${loginSessions}/share";
    };
    # The login screen's KWin tries the cursor theme of its kcminputrc, then
    # "default", then "breeze_cursors" (pointer_input.cpp), and draws no
    # cursor when none is found. Plasma's module installs Breeze and makes it
    # the "default" theme; without Plasma, neither exists. Breeze stays the
    # last place KWin looks, after the system's themes. KWin keeps its own
    # PATH, where it finds Xwayland and the on-screen keyboard.
    systemd.user.services.plasma-login-kwin_wayland = lib.mkIf plasmaLogin {
      overrideStrategy = "asDropin";
      enableDefaultPath = false;
      environment.XCURSOR_PATH = "/run/current-system/sw/share/icons:${pkgs.kdePackages.breeze}/share/icons";
    };
    services.displayManager.defaultSession = sessions.${defaultDesktop};
    # Whichever login screen, it starts that person's default session at
    # boot without asking (Aizome's through greetd's initial session).
    # Preserve existing NixOS settings until Yukimi makes an explicit choice.
    # That choice supersedes ordinary definitions, including when turning it
    # off, while respecting settings deliberately forced by system policy.
    services.displayManager.autoLogin = lib.mkIf (cfg.autoLogin != null) {
      enable = lib.mkOverride 90 (cfg.autoLogin != "");
      user = lib.mkOverride 90 (if cfg.autoLogin == "" then null else cfg.autoLogin);
    };
    services.desktopManager.plasma6.enable = has "plasma";
    services.desktopManager.gnome.enable = has "gnome";
    services.xserver.desktopManager.xfce.enable = has "xfce";
    services.xserver.desktopManager.cinnamon.enable = has "cinnamon";
    services.xserver.desktopManager.mate.enable = has "mate";
    services.xserver.desktopManager.lxqt.enable = has "lxqt";
    calamares.hyprland.enable = has "hyprland";
    programs.tatami.enable = has "tatami";
    # Aizome, Sekka's own desktop, with Tatami's wallpapers: Da Nang at
    # night for Night and Tokyo at dusk for Day, as on its design boards.
    programs.aizome = lib.mkIf (has "aizome") {
      enable = true;
      wallpapers.night = wallpaper "da-nang-at-night.jpg";
      wallpapers.day = wallpaper "tokyo-at-dusk.jpg";
      # Aizome's login screen lists each chosen desktop once, as the others
      # do, and starts X11 ones with startx and NixOS's X server arguments.
      greeter = lib.mkIf aizomeGreeter {
        enable = true;
        sessionDirs = [
          "${loginSessions}/share/wayland-sessions"
          "${loginSessions}/share/xsessions"
        ];
        defaultSession = sessions.${defaultDesktop};
      };
    };
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
    # The keyring's PAM module starts its daemon at login, unlocked by the
    # password, and the Secret Service in it starts on request. XDG autostart
    # asked for it alongside the session's own services, so an early client
    # (Aizome's network agent, the portal) sometimes asked first: D-Bus then
    # started another daemon meanwhile, and the unlocked one crashed on a
    # later call ("gkd_secret_service_get_pkcs11_session: assertion 'client'
    # failed"), leaving a locked keyring. Ask it before the graphical session
    # starts; the request returns once the Secret Service has its bus name.
    systemd.user.services.gnome-keyring-secrets = {
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
      # This snapshot belongs to the user manager's lifetime. Activation must
      # not rerun an inactive oneshot or restart it with a desktop's exports.
      restartIfChanged = false;
      serviceConfig = {
        Type = "oneshot";
        RemainAfterExit = true;
        ExecStart = "${lib.getExe sessionEnv} save %t/calamares-session-env";
      };
    };
    security.pam.services.plasmalogin = lib.mkIf (plasmaLogin && severalDesktops) {
      rules.session.calamares-session-env = sessionEnvRule;
    };
    security.pam.services.gdm-password = lib.mkIf (gdm && severalDesktops) {
      rules.session.calamares-session-env = sessionEnvRule;
    };
    security.pam.services.greetd = lib.mkIf (aizomeGreeter && severalDesktops) {
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
      picture-options='zoom'

      [org.gnome.desktop.screensaver]
      picture-uri='file://${wallpaper "foggy-forest.jpg"}'
      picture-options='zoom'
    '';
    environment.xfce.excludePackages = lib.mkIf (has "xfce") [ pkgs.xfdesktop ];
  };
}
