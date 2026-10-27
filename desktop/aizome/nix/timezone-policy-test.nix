# The published policy follows the effective NixOS setting, including an
# administrator's stronger override and systems that opt out of Aizome's
# default while still leaving the zone mutable.
{ pkgs, self }:
let
  inherit (pkgs) lib;
  policy =
    module:
    let
      config =
        (self.inputs.nixpkgs.lib.nixosSystem {
          system = pkgs.stdenv.hostPlatform.system;
          modules = [
            self.nixosModules.default
            { programs.aizome.enable = true; }
            module
          ];
        }).config;
    in
    config.systemd.services.systemd-timedated.environment.AIZOME_TIMEZONE_MANAGED;
in
assert policy { time.timeZone = "Europe/Paris"; } == "0";
assert
  policy {
    programs.aizome.mutableTimeZone = false;
    time.timeZone = "Europe/Paris";
  } == "1";
assert
  policy {
    programs.aizome.mutableTimeZone = false;
    time.timeZone = null;
  } == "0";
assert policy { time.timeZone = lib.mkForce "Asia/Tokyo"; } == "1";
pkgs.runCommand "aizome-timezone-policy" { } "touch $out"
