{
  pkgs,
  installer,
  references,
  installed,
  airplay,
}:
let
  inherit (pkgs) lib;
  complete = installed.extendModules {
    modules = [
      {
        calamares.installUser = "airplay-check";
        system.stateVersion = "26.11";
      }
    ];
  };
  configurations = {
    installed-default = complete.config;
    live-default = installer.config;
    iso-together = references.together.config;
  }
  // lib.mapAttrs' (
    name: s: lib.nameValuePair "live-${name}" s.configuration
  ) installer.config.specialisation
  // lib.mapAttrs' (name: s: lib.nameValuePair "iso-${name}" s.config) references.systems;
  disabled =
    (complete.extendModules { modules = [ { programs.sekka-airplay.enable = lib.mkForce false; } ]; })
    .config;
  closed =
    (complete.extendModules { modules = [ { programs.sekka-airplay.openFirewall = false; } ]; }).config;
in
assert !(lib.elem airplay disabled.environment.systemPackages);
assert !(lib.elem 60000 disabled.networking.firewall.allowedUDPPorts);
assert lib.elem airplay closed.environment.systemPackages;
assert !(lib.elem 60000 closed.networking.firewall.allowedUDPPorts);
assert closed.environment.etc."sekka/airplay/open-firewall".text == "false\n";
pkgs.writeText "sekka-airplay-packages.json" (
  builtins.toJSON (
    lib.mapAttrs (
      _: config:
      assert config.programs.sekka-airplay.enable;
      assert config.programs.sekka-airplay.package.outPath == airplay.outPath;
      assert lib.elem airplay config.environment.systemPackages;
      assert config.environment.etc."sekka/airplay/open-firewall".text == "true\n";
      assert config.services.avahi.enable;
      assert lib.all (port: lib.elem port config.networking.firewall.allowedUDPPorts) [
        5353
        60000
        60001
        60002
      ];
      {
        package = airplay.outPath;
        udpPorts = config.networking.firewall.allowedUDPPorts;
      }
    ) configurations
  )
)
