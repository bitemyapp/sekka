# Chanoma comes with Aizome, and nothing comes with Chanoma unasked: no
# browser (Google Chrome may not be passed on, and the media allows unfree
# packages), no change to how Bluetooth devices pair, nothing started.
# Evaluated for Aizome's module alone, the live media, and the installed
# Aizome system the media carries. A configuration check: it builds and
# runs nothing of Chanoma.
{
  pkgs,
  inputs,
  installer,
  references,
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
          # As the media and most installed systems are: where a browser
          # by default would have been Google Chrome.
          nixpkgs.config.allowUnfree = true;
          services.openssh.enable = true;
        }
        extra
      ];
    }).config;
  on = alone { };
  off = alone { programs.aizome.chanoma.enable = false; };
  television = alone { programs.aizome.chanoma.controllersPairAgain = true; };
  media = references.systems.aizome.config;

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
  };
in
# With Aizome, and with nothing else.
assert hasChanoma on;
assert on.programs.aizome.chanoma.browser == null;
assert browsers on == [ ];
assert !(bluetooth on ? JustWorksRepairing) && !(bluetooth on ? FastConnectable);
assert !(on.systemd.user.services ? aizome-chanoma);
# The TV account's PIN is no way in over the network, wherever Settings
# may make that account.
assert on.services.openssh.settings.DenyUsers == [ "tv" ];
# A television asks for its controllers to pair again unasked.
assert (bluetooth television).JustWorksRepairing == "always";
assert (bluetooth television).FastConnectable;
# Left out, all of it goes.
assert !hasChanoma off;
assert off.services.openssh.settings.DenyUsers == null;
# The live media and the installed system it carries allow unfree packages:
# Chanoma is there, and no browser came with it.
assert installer.config.nixpkgs.config.allowUnfree && media.nixpkgs.config.allowUnfree;
assert hasChanoma installer.config && browsers installer.config == [ ];
assert hasChanoma media && browsers media == [ ];
pkgs.writeText "aizome-chanoma-defaults.json" (
  builtins.toJSON {
    alone = summary on;
    live = summary installer.config;
    media = summary media;
  }
)
