# Where terminals open in fish, set up as Omarchy sets it up: wherever Aizome
# is installed, whatever applications were chosen; not with Tatami or another
# desktop alone (Omarchy's own fish is opt-in), unless the system chose the
# catalog's former "fish" or turns calamares.fish on, as Yukimi offers.
# Evaluation only; checks.fish logs in to a system with it.
{
  pkgs,
  inputs,
  applicationCatalog,
}:
let
  inherit (pkgs) lib;
  installed =
    settings:
    (inputs.nixpkgs.lib.nixosSystem {
      system = pkgs.stdenv.hostPlatform.system;
      inherit (applicationCatalog) specialArgs;
      modules = [
        inputs.calamares.nixosModules.default
        {
          calamares.installUser = "alice";
          calamares.applications = lib.mkDefault [ ];
          users.users.alice.isNormalUser = true;
          system.stateVersion = "26.11";
          boot.loader.grub.enable = false;
          fileSystems."/" = {
            device = "none";
            fsType = "tmpfs";
          };
        }
        settings
      ];
    }).config;
  desktops = list: {
    calamares.desktops = list;
    calamares.defaultDesktop = builtins.head list;
  };
  aizome = installed (desktops [ "aizome" ]);
  together = installed (desktops [
    "tatami"
    "aizome"
  ]);
  tatami = installed (desktops [ "tatami" ]);
  plasma = installed (desktops [ "plasma" ]);
  # A Tatami system that chose fish among the applications before it came
  # with Aizome keeps it.
  chosen = installed {
    imports = [ (desktops [ "tatami" ]) ];
    calamares.applications = [
      "fish"
      "firefox"
    ];
  };
  declined = installed {
    imports = [ (desktops [ "aizome" ]) ];
    calamares.fish.enable = false;
  };
  init = c: builtins.unsafeDiscardStringContext c.programs.bash.interactiveShellInit;
  handsOver =
    c:
    lib.hasInfix (builtins.unsafeDiscardStringContext "exec ${lib.getExe c.programs.fish.package} ") (
      init c
    );
  fish =
    c:
    c.programs.fish.enable
    && handsOver c
    && lib.elem c.programs.fish.package c.environment.systemPackages;
  bash = c: !c.programs.fish.enable && !(lib.hasInfix "fish" (init c));
  json = file: builtins.fromJSON (builtins.unsafeDiscardStringContext file.text);
  manifest = c: json c.environment.etc."installer-applications.json";
  choice =
    c:
    lib.findFirst (choice: choice.setting == "calamares.fish.enable") null
      (json c.environment.etc."yukimi/system.json").choices;
in
assert fish aizome && fish together && fish chosen;
assert bash tatami && bash plasma && bash declined;
# fish is no longer among the applications, for Genkan or Yukimi's Discover.
assert !(lib.any (app: app.id == "fish") applicationCatalog.catalog);
assert (manifest aizome).selected == [ ] && (manifest chosen).selected == [ "firefox" ];
# Yukimi offers to turn it off with Aizome, and on without.
assert (choice aizome).value == "true" && (choice declined).value == "false";
assert (choice tatami).value == "false" && (choice chosen).value == "true";
assert
  (choice aizome).boolean
  &&
    map (option: option.id) (choice aizome).options == [
      "true"
      "false"
    ];
# Force module assertions, not just the options above.
assert builtins.all (c: c.system.build.toplevel.drvPath != "") [
  aizome
  tatami
  chosen
];
pkgs.writeText "fish-default.json" (
  builtins.toJSON (
    lib.mapAttrs (_: c: c.programs.fish.enable) {
      inherit
        aizome
        together
        tatami
        plasma
        chosen
        declined
        ;
    }
  )
)
