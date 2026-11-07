# SPDX-License-Identifier: GPL-3.0-or-later
# First-boot setup (calamares.firstBootSetup; docs/apple-silicon/first-boot-
# setup.md): an installed image with no account sets itself up for its owner
# on Aizome's login screen. The login screen's own user, locked and
# unprivileged, asks; sekka-setup-helper, a root service only it can reach,
# makes the account and applies the choices as the running system's state,
# without building a generation, and writes the same choices to
# /etc/nixos/setup.nix, which configuration.nix imports, so the first
# update builds exactly what was chosen.
#
# Every installed system imports this module (installer/genkan/rust/system);
# it does nothing unless enabled. It sets what makes those choices legal at
# run time on NixOS: no host name, time zone or locale fixed by the
# configuration (each a default the setup.nix it writes overrides), localectl
# and hostnamectl allowed to write theirs, and every locale setup offers
# installed, in an order no choice changes. Once setup.nix is in the
# configuration (`applied`), the helper, its socket, the login screen's way
# to it and the module's assertions are gone.
{
  config,
  lib,
  options,
  pkgs,
  utils,
  ...
}:
let
  cfg = config.calamares.firstBootSetup;
  # Genkan's locales and layouts (the helper's tests prove these are its).
  offered = lib.importJSON ./offered.json;
  has = path: lib.hasAttrByPath path options;
  apple = config.calamares.appleSilicon or { };
  # The desktops Genkan knows (calamares_nixos::Desktop), as installed.
  known = [
    "aizome"
    "plasma"
    "gnome"
    "xfce"
    "hyprland"
    "tatami"
  ];
  helperConfig = {
    greeter_user = "greeter";
    state_dir = "/var/lib/sekka-setup";
    nixos_dir = "/etc/nixos";
    inherit (cfg) handoff;
    zoneinfo = "/etc/zoneinfo";
    home_base = "/home";
    shell = utils.toShellPath config.users.defaultUserShell;
    primary_group = "users";
    inherit (cfg) groups;
    declared_groups = cfg.declaredGroups;
    desktops = cfg.desktops;
    default_desktop = cfg.defaultDesktop;
    keyboard = config.services.xserver.xkb.layout;
    locale = config.i18n.defaultLocale;
    inherit (cfg) device;
    greeter_keyboard = keyboardFile;
    verify = cfg.verifyConfiguration;
    state_version = config.system.stateVersion;
    tools = {
      useradd = "${pkgs.shadow}/bin/useradd";
      usermod = "${pkgs.shadow}/bin/usermod";
      chpasswd = "${pkgs.shadow}/bin/chpasswd";
      hostnamectl = "${config.systemd.package}/bin/hostnamectl";
      timedatectl = "${config.systemd.package}/bin/timedatectl";
      localectl = "${config.systemd.package}/bin/localectl";
      systemctl = "${config.systemd.package}/bin/systemctl";
      nix = "${config.nix.package}/bin/nix";
    };
  };
  keyboardFile = "/etc/aizome/keyboard.toml";
  socket = "/run/sekka-setup.sock";
  waiting = cfg.enable && !cfg.applied;
  # Every offered locale, in one order whichever is the default: glibc's
  # locale archive is built from this list as written, so the image's serves
  # every choice and setup.nix's language rebuilds nothing. Anything else
  # the configuration names (nixpkgs' own default list) follows.
  locales = lib.unique (
    [ "C.UTF-8/UTF-8" ]
    ++ map (l: "${l}/UTF-8") offered.locales
    ++ options.i18n.supportedLocales.default
  );
  # /etc/vconsole.conf as NixOS's console module writes it (console.nix's
  # vconsoleConf, the same file), which it leaves out under imperativeLocale.
  vconsoleConf = pkgs.writeText "vconsole.conf" ''
    KEYMAP=${config.console.keyMap}
    ${lib.optionalString (config.console.font != null) "FONT=${config.console.font}"}
  '';
in
{
  options.calamares.firstBootSetup = {
    enable = lib.mkEnableOption ''
      first-boot setup on Aizome's login screen: with no account on the
      system, the login screen asks for the owner's language, keyboard,
      time zone, account and desktop, and a root helper applies them and
      writes /etc/nixos/setup.nix'';
    applied = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = ''
        The configuration includes setup's choices (setup.nix sets this):
        setup is over, and its helper and socket are left out.
      '';
    };
    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.callPackage ./package.nix { };
      defaultText = lib.literalExpression "pkgs.callPackage ./package.nix { }";
      description = "sekka-setup-helper.";
    };
    handoff = lib.mkOption {
      type = lib.types.str;
      default = "${config.boot.loader.efi.efiSysMountPoint}/sekka/install.json";
      defaultText = lib.literalExpression ''"''${config.boot.loader.efi.efiSysMountPoint}/sekka/install.json"'';
      description = ''
        The Mac installer's handoff (`sekka.mac.handoff/1`), whose hints
        about macOS's language, keyboard, time zone and names setup starts
        from. Setup works without one.
      '';
    };
    device = lib.mkOption {
      type = lib.types.str;
      default = if apple.enable or false then "Mac" else "computer";
      defaultText = lib.literalExpression ''if config.calamares.appleSilicon.enable then "Mac" else "computer"'';
      description = ''What setup calls the computer ("Set up your Mac").'';
    };
    desktops = lib.mkOption {
      type = lib.types.listOf (lib.types.enum known);
      default = lib.filter (d: lib.elem d known) (config.calamares.desktops or [ "aizome" ]);
      defaultText = lib.literalExpression "config.calamares.desktops";
      description = "The installed desktops setup offers.";
    };
    defaultDesktop = lib.mkOption {
      type = lib.types.enum known;
      default = config.calamares.defaultDesktop or (lib.head cfg.desktops);
      defaultText = lib.literalExpression "config.calamares.defaultDesktop";
      description = "The desktop setup starts with.";
    };
    declaredGroups = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [
        "networkmanager"
        "wheel"
      ];
      description = ''
        The groups setup.nix gives the account, as Genkan's configuration
        gives the account it makes (installer/genkan/rust/src/config.rs).
      '';
    };
    groups = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default =
        cfg.declaredGroups
        # nix/apple-silicon/module.nix gives calamares.installUser the M3
        # Pro's render node (graphicsUsers); setup.nix names the account
        # installUser, and the account has it at once.
        ++ lib.optional ((apple.enable or false) && (apple.m3ProGraphics or false)) "render";
      defaultText = lib.literalExpression ''declaredGroups ++ [ "render" ] (on the M3 Pro's GPU)'';
      description = ''
        Every group the account gets when setup makes it: what the installed
        configuration gives it once setup.nix is built, so that build changes
        none of them.
      '';
    };
    verifyConfiguration = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = ''
        Before finishing, evaluate /etc/nixos/flake.nix, offline, for the
        new host name's `nixosConfigurations` entry (as Yukimi and
        nixos-rebuild will), proving the configuration with setup.nix still
        evaluates. Skipped where there is no flake.
      '';
    };
  };

  config = lib.mkIf cfg.enable (
    lib.mkMerge [
      {
        # No account can log in until setup makes the owner's.
        users.users.root.initialHashedPassword = lib.mkDefault "!";
        # What setup chooses: defaults that setup.nix overrides, and until
        # then the running system's to change.
        networking.hostName = lib.mkDefault "";
        time.timeZone = lib.mkDefault null;
        i18n.defaultLocale = lib.mkDefault "en_US.UTF-8";
        services.xserver.xkb.layout = lib.mkDefault "us";
        # localectl writes /etc/locale.conf (and Settings' language changes
        # last); every locale setup offers is installed, so no choice
        # needs a rebuild to work.
        i18n.imperativeLocale = true;
        i18n.extraLocales = map (l: "${l}/UTF-8") offered.locales;
        i18n.supportedLocales = lib.mkDefault locales;
        # The language is the system's own; the keyboard is the
        # configuration's (setup.nix's services.xserver.xkb.layout), and so
        # the console's (console.useXkbConfig): /etc/vconsole.conf stays
        # NixOS's, which imperativeLocale would leave as first seeded, so
        # the console's keymap follows setup's keyboard at the first rebuild.
        environment.etc."vconsole.conf" = lib.mkIf config.console.enable { source = vconsoleConf; };
        systemd.services.reload-systemd-vconsole-setup.restartTriggers = lib.mkIf config.console.enable [
          vconsoleConf
        ];
        # With no host name configured, hostnamed keeps the static one in
        # /etc/hostname (NixOS points it at /etc/static/hostname, which
        # doesn't exist then), so hostnamectl's change lasts, and boot reads it.
        systemd.services.systemd-hostnamed.environment.SYSTEMD_ETC_HOSTNAME = lib.mkIf (
          config.networking.hostName == ""
        ) (lib.mkForce "/etc/hostname");
      }
      (lib.optionalAttrs (has [ "calamares" "installUser" ]) {
        # Before setup there's no person's account: the login screen's
        # stands in, so modules that give the installer's account something
        # name an account that exists. setup.nix names the person's.
        calamares.installUser = lib.mkDefault "greeter";
      })
      (lib.mkIf waiting {
        # What setup needs until it is over; afterwards the owner's account
        # exists and these are theirs to choose.
        assertions = [
          {
            assertion = config.users.mutableUsers;
            message = "calamares.firstBootSetup makes the owner's account imperatively: it needs users.mutableUsers.";
          }
          {
            assertion = config.programs.aizome.greeter.enable or false;
            message = "calamares.firstBootSetup runs on Aizome's login screen: enable programs.aizome.greeter.";
          }
          {
            assertion = !config.services.displayManager.autoLogin.enable;
            message = "calamares.firstBootSetup: a computer set up at first boot has no account to log in automatically.";
          }
        ];
        environment.etc."sekka-setup/helper.json".text = builtins.toJSON helperConfig;
        users.groups.sekka-setup = { };
        # Only the login screen's account reaches the helper.
        users.users.greeter.extraGroups = [ "sekka-setup" ];
        programs.aizome.greeter.setup = {
          inherit socket;
          logo = ../../../docs/brand/logos/sekka-lockup-night.svg;
        };
        programs.aizome.greeter.keyboardFile = keyboardFile;
        systemd.sockets.sekka-setup = {
          description = "Sekka's first-boot setup, for the login screen";
          wantedBy = [ "sockets.target" ];
          listenStreams = [ socket ];
          socketConfig = {
            SocketUser = "root";
            SocketGroup = "sekka-setup";
            SocketMode = "0660";
            Accept = false;
            RemoveOnStop = true;
          };
        };
        systemd.services.sekka-setup = {
          description = "Sekka's first-boot setup helper";
          requires = [ "sekka-setup.socket" ];
          after = [ "sekka-setup.socket" ];
          # Setting the host name, time zone and locale asks systemd's
          # services; the account, shadow's tools; the check, the Nix daemon.
          wants = [ "nix-daemon.socket" ];
          serviceConfig = {
            ExecStart = "${cfg.package}/bin/sekka-setup-helper serve";
            NoNewPrivileges = true;
            PrivateTmp = true;
            PrivateNetwork = false;
            ProtectKernelTunables = true;
            ProtectKernelModules = true;
            ProtectKernelLogs = true;
            ProtectControlGroups = true;
            ProtectClock = true;
            RestrictAddressFamilies = [ "AF_UNIX" ];
            RestrictNamespaces = true;
            RestrictRealtime = true;
            RestrictSUIDSGID = true;
            LockPersonality = true;
            SystemCallArchitectures = "native";
            UMask = "0077";
          };
        };
        systemd.tmpfiles.rules = [ "d /var/lib/sekka-setup 0700 root root -" ];
      })
    ]
  );
}
