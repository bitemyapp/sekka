# SPDX-License-Identifier: GPL-3.0-or-later
# First-boot setup's outputs for one system (flake.nix:
# `nix build .#legacyPackages.SYSTEM.first-boot.NAME`):
#
# - helper: sekka-setup-helper, with its tests;
# - configuration: a Sekka installed system evaluated before and after
#   setup.nix (test-data/setup.nix, what the helper writes), proving the
#   account's groups, host name, locale, keyboard and desktop agree with
#   what setup applied at run time, and that the result evaluates;
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
  # image.nix), its account left to setup.
  installed =
    extra:
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
        {
          calamares = {
            applications = [ "firefox" ];
            desktops = [
              "aizome"
              "tatami"
            ];
            defaultDesktop = "aizome";
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
          users.mutableUsers = true;
          nixpkgs.config.allowUnfree = true;
          system.stateVersion = "26.11";
        }
        extra
      ];
    };
  before = (installed { }).config;
  after = (installed ./test-data/setup.nix).config;
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
      lib.all (l: lib.elem "${l}/UTF-8" before.i18n.supportedLocales) (lib.importJSON ./offered.json).locales
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
      && after.services.displayManager.defaultSession == "aizome"
      && after.fileSystems."/".device == "/dev/disk/by-partuuid/63d89be1-72f7-4aff-9679-09cafa7e0c2d"
    );
    assert check "once applied, the helper and its socket are gone" (
      !(after.systemd.sockets ? sekka-setup) && after.programs.aizome.greeter.setup.socket == null
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
