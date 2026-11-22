# Chanoma comes with Aizome, whole, with nothing to turn on: the app, its
# browser where the system may have Google Chrome, and its controllers
# pairing again by themselves. Evaluated for Aizome's module alone (with
# unfree packages allowed and refused), the live media, and the installed
# Aizome system the media carries. The installed
# system the binary cache publishes refuses unfree packages, and has no
# Chrome. A configuration check: it builds and runs nothing of Chanoma.
{
  pkgs,
  inputs,
  installer,
  references,
  applicationCatalog,
}:
let
  inherit (pkgs) lib;
  alone =
    extra:
    (inputs.nixpkgs.lib.nixosSystem {
      system = pkgs.stdenv.hostPlatform.system;
      modules = [
        inputs.aizome.nixosModules.default
        {
          programs.aizome.enable = true;
          services.openssh.enable = true;
        }
        extra
      ];
    }).config;
  # As the media and most installed systems are.
  on = alone { nixpkgs.config.allowUnfree = true; };
  # As the systems the binary cache publishes are.
  free = alone { nixpkgs.config.allowUnfree = false; };
  off = alone {
    nixpkgs.config.allowUnfree = true;
    programs.aizome.chanoma.enable = false;
  };
  media = references.systems.aizome.config;
  # The same installed system as the binary cache publishes it
  # (packages.installed-systems).
  published =
    (import ./reference-systems.nix {
      inherit inputs applicationCatalog;
      desktops = [ "aizome" ];
      free = true;
    }).systems.aizome.config;

  packages = config: builtins.filter builtins.isAttrs config.environment.systemPackages;
  # The package's `chanoma` output: the app, its entry and the TV session.
  hasChanoma = config: lib.any (p: (p.outputName or null) == "chanoma") (packages config);
  browsers =
    config:
    builtins.filter (name: name == "google-chrome" || lib.hasPrefix "chromium" name) (
      map lib.getName (packages config)
    );
  bluetooth = config: config.hardware.bluetooth.settings.General or { };
  summary = config: {
    chanoma = hasChanoma config;
    browsers = browsers config;
    unfree = config.nixpkgs.config.allowUnfree;
    pairsAgain = (bluetooth config).JustWorksRepairing or null;
  };
in
# With Aizome, and with what its services and controllers need.
assert hasChanoma on;
assert browsers on == [ "google-chrome" ];
# A Bluetooth device that forgot the computer pairs again unasked. That is
# decided on purpose (desktop/aizome/docs/chanoma.md, "Decided on
# purpose"): a change that takes it out is not a fix, and fails here.
assert (bluetooth on).JustWorksRepairing == "always" && (bluetooth on).FastConnectable;
# The desktop entry uses the catalog's keyring wrapper. The TV keeps
# its own browser profiles and receives the raw executable by store path.
assert lib.all (p: toString p != toString on.programs.aizome.chanoma.browser) (packages on);
assert builtins.elem "CHANOMA_BROWSER=${lib.getExe on.programs.aizome.chanoma.browser}" (
  lib.splitString "\n" on.environment.etc."xdg/aizome/environment".text
);
# Nothing starts by itself, and the TV account's PIN is no way in over the
# network, wherever Settings may make that account.
assert !(on.systemd.user.services ? aizome-chanoma);
assert on.services.openssh.settings.DenyUsers == null;
assert builtins.elem "aizome-tv-kiosk" on.services.openssh.settings.DenyGroups;
# Where unfree packages are refused it still comes, without the browser.
assert hasChanoma free;
assert free.programs.aizome.chanoma.browser == null && browsers free == [ ];
# Left out, all of it goes.
assert !hasChanoma off && browsers off == [ ];
assert !(bluetooth off ? JustWorksRepairing) && !(bluetooth off ? FastConnectable);
assert off.services.openssh.settings.DenyUsers == null;
assert builtins.elem "aizome-tv-kiosk" off.services.openssh.settings.DenyGroups;
# The live media and the installed system it carries are as the module
# alone is.
assert installer.config.nixpkgs.config.allowUnfree && media.nixpkgs.config.allowUnfree;
assert hasChanoma installer.config && browsers installer.config == [ "google-chrome" ];
assert hasChanoma media && browsers media == [ "google-chrome" ];
assert (bluetooth media).JustWorksRepairing == "always" && (bluetooth media).FastConnectable;
# The published system refuses unfree packages: Chanoma, and no Chrome.
assert !published.nixpkgs.config.allowUnfree;
assert hasChanoma published;
assert published.programs.aizome.chanoma.browser == null && browsers published == [ ];
pkgs.writeText "aizome-chanoma-defaults.json" (
  builtins.toJSON {
    alone = summary on;
    free = summary free;
    live = summary installer.config;
    media = summary media;
    published = summary published;
  }
)
