# GNOME's file manager and calculator, which Tatami brings, are listed in
# Tatami alone, unless GNOME is installed too: then they're GNOME's own,
# unchanged, and every desktop lists them.
{
  pkgs,
  inputs,
  applicationCatalog,
}:
let
  inherit (pkgs) lib;
  installed =
    desktops:
    inputs.nixpkgs.lib.nixosSystem {
      system = pkgs.stdenv.hostPlatform.system;
      inherit (applicationCatalog) specialArgs;
      modules = [
        inputs.calamares.nixosModules.default
        {
          calamares.desktops = desktops;
          calamares.applications = [ ];
          calamares.installUser = "demo";
        }
      ];
    };
  entries = {
    nautilus = "org.gnome.Nautilus.desktop";
    gnome-calculator = "org.gnome.Calculator.desktop";
  };
  gnomeApps =
    system:
    lib.filter (p: entries ? ${lib.getName p}) (lib.unique system.config.environment.systemPackages);
  tatami = gnomeApps (installed [
    "aizome"
    "tatami"
  ]);
  withGnome =
    let
      system = installed [
        "aizome"
        "tatami"
        "gnome"
      ];
    in
    map (p: {
      inherit p;
      own = system.pkgs.${lib.getName p};
    }) (gnomeApps system);
in
assert lib.length tatami == 2;
assert lib.length withGnome == 2;
assert builtins.all ({ p, own }: p.outPath == own.outPath) withGnome;
pkgs.runCommand "tatami-gnome-apps" { } ''
  ${lib.concatMapStrings (p: ''
    test -x ${p}/bin/${lib.getName p}
    awk '/^\[/ { group = $0 }
      group == "[Desktop Entry]" && $0 == "OnlyShowIn=Tatami;" { found = 1 }
      END { exit !found }' ${p}/share/applications/${entries.${lib.getName p}}
  '') tatami}
  touch $out
''
