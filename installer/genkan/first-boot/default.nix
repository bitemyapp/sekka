# SPDX-License-Identifier: GPL-3.0-or-later
# First-boot setup's outputs for one system (flake.nix:
# `nix build .#legacyPackages.SYSTEM.first-boot.NAME`):
#
# - helper: sekka-setup-helper, with its tests;
# - configuration: a Sekka installed system evaluated before and after
#   setup (test-data/setup.nix and configuration.after.nix, what the helper
#   writes), proving the account's groups, shell, host name, locale,
#   keyboard and desktop agree with what setup applied at run time, that
#   the default desktop is configuration.nix's alone, that setup.nix leaves
#   the disk, the system's packages and its locales as they were and the
#   console keymap follows the keyboard, that the module asserts nothing
#   once it's applied, and that the result evaluates;
# - vm-test: the login screen's setup in a disposable VM, typed at as a
#   person would (vm-test.nix).
{
  nixpkgs,
  inputs,
  system,
}:
let
  pkgs = nixpkgs.legacyPackages.${system};
  inherit (nixpkgs) lib;
  helper = pkgs.callPackage ./package.nix { };
  # An installed Sekka as the Mac image would be (nix/apple-silicon/
  # image.nix), its account left to setup, with the part of configuration.nix
  # setup changes as `configuration` has it (test-data/configuration.nix as
  # the image ships it, configuration.after.nix as setup leaves it).
  installation =
    configuration: extra:
    nixpkgs.lib.nixosSystem {
      inherit system;
      specialArgs = {
        applicationPkgs = import inputs.applications {
          inherit system;
          config.allowUnfree = true;
        };
        aiPackages = inputs.ai-apps.packages.${system};
        ompPackage = inputs.omp.packages.${system}.omp;
      };
      modules = [
        inputs.determinate.nixosModules.default
        inputs.calamares.nixosModules.default
        configuration
        {
          calamares = {
            applications = [ "firefox" ];
            appleSilicon = lib.mkIf (system == "aarch64-linux") {
              enable = true;
              t6030Handoff = true;
            };
            firstBootSetup = {
              enable = true;
              package = helper;
            };
          };
          boot.loader.systemd-boot.enable = true;
          fileSystems."/" = {
            device = "/dev/disk/by-label/sekka-root";
            fsType = "ext4";
          };
          fileSystems."/boot" = {
            device = "/dev/disk/by-uuid/5ECC-A5E1";
            fsType = "vfat";
          };
          networking.networkmanager.enable = true;
          console.useXkbConfig = true;
          users.mutableUsers = true;
          nixpkgs.config.allowUnfree = true;
          system.stateVersion = "26.11";
        }
      ]
      ++ extra;
    };
  installed = extra: (installation ./test-data/configuration.nix extra).config;
  before = installed [ ];
  afterSetup = installation ./test-data/configuration.after.nix [ ./test-data/setup.nix ];
  after = afterSetup.config;
  offered = lib.importJSON ./offered.json;
  helperConfig = builtins.fromJSON (
    builtins.unsafeDiscardStringContext before.environment.etc."sekka-setup/helper.json".text
  );
  # What the owner may choose once set up, and the image may not.
  owners = {
    calamares.loginScreen = "gdm";
    services.displayManager.autoLogin = {
      enable = true;
      user = "ada";
    };
    users.mutableUsers = lib.mkForce false;
  };
  # The module's own assertions that fail (only a failing one's message is
  # sure to evaluate).
  ours =
    config:
    lib.filter (lib.hasPrefix "calamares.firstBootSetup") (
      map (a: a.message) (lib.filter (a: !a.assertion) config.assertions)
    );
  sorted = l: lib.sort lib.lessThan (lib.unique l);
  check = what: ok: if ok then true else throw "first-boot setup's configuration: ${what}";
in
{
  inherit helper;

  configuration =
    assert check "the image's account groups are those setup.nix's account gets" (
      sorted before.calamares.firstBootSetup.groups == sorted after.users.users.ada.extraGroups
    );
    assert check "the image leaves the host name to setup" (before.networking.hostName == "");
    assert check "hostnamed keeps setup's host name" (
      before.systemd.services.systemd-hostnamed.environment.SYSTEMD_ETC_HOSTNAME == "/etc/hostname"
    );
    assert check "the image's locales include every one setup offers" (
      lib.all (l: lib.elem "${l}/UTF-8" before.i18n.supportedLocales) offered.locales
    );
    # glibc's archive is built from the list as written: the language
    # chosen mustn't reorder it, or every choice but the first rebuilds it.
    assert check "every language setup offers keeps the image's glibc locales" (
      lib.all (
        l: (installed [ { i18n.defaultLocale = l; } ]).i18n.glibcLocales.drvPath == before.i18n.glibcLocales.drvPath
      ) offered.locales
    );
    assert check "the image's login screen asks the helper" (
      before.programs.aizome.greeter.setup.socket == "/run/sekka-setup.sock"
    );
    assert check "the image has no usable password" (
      lib.all (u: (u.hashedPassword or null) == null && u.password == null && u.initialPassword == null) (
        lib.attrValues before.users.users
      )
    );
    assert check "setup.nix's account and choices are the configuration's" (
      after.calamares.installUser == "ada"
      && after.users.users.ada.uid == 1000
      && after.users.users.ada.isNormalUser
      && after.users.users.ada.hashedPassword == null
      && after.networking.hostName == "analytical"
      && after.i18n.defaultLocale == "en_GB.UTF-8"
      && after.services.xserver.xkb.layout == "gb"
      && after.services.displayManager.defaultSession == "tatami"
    );
    assert check "setup.nix names the shell the helper makes the account with" (
      after.users.users.ada.shell == helperConfig.shell
    );
    # So Yukimi's edit of configuration.nix's line is the only one to make.
    assert check "the default desktop is configuration.nix's, and setup.nix doesn't name it" (
      lib.elem (toString ./test-data/configuration.after.nix) (
        map toString afterSetup.options.calamares.defaultDesktop.files
      )
      && !(lib.elem (toString ./test-data/setup.nix) (map toString afterSetup.options.calamares.defaultDesktop.files))
    );
    assert check "setup.nix leaves the disk to the configuration (identity.nix on a Mac)" (
      after.fileSystems."/".device == before.fileSystems."/".device
      && after.fileSystems."/boot".device == before.fileSystems."/boot".device
    );
    assert check "setup.nix changes none of the system's packages" (
      before.system.path.drvPath == after.system.path.drvPath
    );
    assert check "the console's keymap is the configuration's, and follows setup.nix's keyboard" (
      before.environment.etc ? "vconsole.conf"
      && toString before.environment.etc."vconsole.conf".source
        != toString after.environment.etc."vconsole.conf".source
    );
    assert check "once applied, the helper and its socket are gone" (
      !(after.systemd.sockets ? sekka-setup) && after.programs.aizome.greeter.setup.socket == null
    );
    assert check "once applied, what setup left in its state directory goes" (
      lib.elem "r /var/lib/sekka-setup/credential" after.systemd.tmpfiles.rules
      && !(lib.elem "r /var/lib/sekka-setup/credential" before.systemd.tmpfiles.rules)
    );
    assert check "once applied, the module asserts nothing (the login screen and accounts are the owner's)" (
      lib.length (ours (installed [ owners ])) == 3
      && ours (installed [
        ./test-data/setup.nix
        owners
      ]) == [ ]
    );
    # Both systems evaluated whole, to their derivations; neither is built.
    pkgs.writeText "first-boot-configuration-${system}" (
      builtins.unsafeDiscardStringContext ''
        before: ${before.system.build.toplevel.drvPath}
        after: ${after.system.build.toplevel.drvPath}
      ''
    );

  vm-test = import ./vm-test.nix {
    inherit pkgs helper;
    aizome = inputs.aizome;
  };
}
