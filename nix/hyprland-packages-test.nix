{
  pkgs,
  inputs,
  applicationCatalog,
  aquamarine,
  hyprland,
}:
let
  inherit (pkgs) lib;
  system = pkgs.stdenv.hostPlatform.system;
  references = import ./reference-systems.nix {
    inherit inputs applicationCatalog;
    desktops = [
      "hyprland"
      "tatami"
    ];
  };
  evaluate =
    modules:
    inputs.nixpkgs.lib.nixosSystem {
      inherit system modules;
      inherit (applicationCatalog) specialArgs;
    };
  configurations = {
    # These are the actual reference systems embedded on the ISO.
    iso-hyprland = references.systems.hyprland;
    iso-tatami = references.systems.tatami;
    # The public installed-system entry point, with both imports present.
    installed = evaluate [
      inputs.calamares.nixosModules.default
      {
        calamares.desktops = [
          "tatami"
          "hyprland"
        ];
      }
    ];
    standalone-tatami = evaluate [
      inputs.tatami.nixosModules.default
      { programs.tatami.enable = true; }
    ];
    standalone-hyprland = evaluate [
      ../installer/genkan/rust/system/hyprland.nix
      { calamares.hyprland.enable = true; }
    ];
  };
  paths = lib.mapAttrs (
    name: evaluated:
    assert evaluated.config.programs.hyprland.enable;
    assert evaluated.config.programs.hyprland.package.outPath == hyprland.outPath;
    assert evaluated.pkgs.aquamarine.outPath == aquamarine.outPath;
    {
      hyprland = evaluated.config.programs.hyprland.package.outPath;
      aquamarine = evaluated.pkgs.aquamarine.outPath;
    }
  ) configurations;
in
pkgs.writeText "sekka-hyprland-packages.json" (builtins.toJSON paths)
