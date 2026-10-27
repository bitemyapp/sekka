# Evaluate the installed Aizome integration, including the JSON consumed by
# Settei and Yukimi. No running system or boot configuration is changed.
{ pkgs, inputs }:
let
  inherit (pkgs) lib;
  system =
    modules:
    (inputs.nixpkgs.lib.nixosSystem {
      system = pkgs.stdenv.hostPlatform.system;
      modules = modules ++ [
        {
          system.stateVersion = "26.11";
          boot.loader.grub.enable = false;
          fileSystems."/" = {
            device = "none";
            fsType = "tmpfs";
          };
        }
      ];
    }).config;
  installed =
    extra:
    system [
      inputs.calamares.nixosModules.desktops
      {
        calamares.desktops = [ "aizome" ];
        calamares.defaultDesktop = "aizome";
      }
      extra
    ];
  older = installed { };
  on = installed { calamares.bootSplash.enable = true; };
  off = installed { calamares.bootSplash.enable = false; };
  # The splash module remains usable by standalone consumers without Yukimi.
  standalone = system [
    ../installer/genkan/rust/system/boot-splash
    { calamares.bootSplash.enable = true; }
  ];
  facts =
    c:
    builtins.fromJSON (builtins.unsafeDiscardStringContext c.environment.etc."yukimi/system.json".text);
  splash = c: lib.filter (choice: choice.setting == "calamares.bootSplash.enable") (facts c).choices;
  offered =
    value: c:
    let
      choices = splash c;
    in
    builtins.length choices == 1
    && (builtins.head choices).boolean
    && !(builtins.head choices).many
    && (builtins.head choices).value == value
    &&
      map (option: option.id) (builtins.head choices).options == [
        "true"
        "false"
      ];
in
assert offered "false" older;
assert offered "false" off;
assert offered "true" on;
assert !older.boot.plymouth.enable && !off.boot.plymouth.enable;
assert on.boot.plymouth.enable && on.boot.plymouth.theme == "sekka";
assert lib.elem "splash" on.boot.kernelParams && lib.elem "quiet" on.boot.kernelParams;
assert !(lib.elem "splash" off.boot.kernelParams) && !(lib.elem "quiet" off.boot.kernelParams);
assert
  map (choice: choice.setting) (facts on).choices == [
    "calamares.desktops"
    "calamares.defaultDesktop"
    "calamares.loginScreen"
    "calamares.bootSplash.enable"
  ];
# Both interfaces and the helper must come from this checkout's integration.
assert
  on.programs.aizome.package == inputs.aizome.packages.${pkgs.stdenv.hostPlatform.system}.aizome;
assert
  on.programs.yukimi.package == inputs.yukimi.packages.${pkgs.stdenv.hostPlatform.system}.yukimi;
assert standalone.boot.plymouth.enable;
assert !(standalone.environment.etc ? "yukimi/system.json");
# Force module assertions and lazy etc/system entries, not just the options above.
assert builtins.all (c: c.system.build.toplevel.drvPath != "" && c.system.build.etc.drvPath != "") [
  older
  on
  off
  standalone
];
pkgs.writeText "boot-splash-choice.json" (
  builtins.toJSON {
    older = splash older;
    on = splash on;
    off = splash off;
  }
)
