# The default desktop package sets must expose one Hotaru and no Foot.
{
  pkgs,
  inputs,
  installer,
  applicationCatalog,
}:
let
  inherit (pkgs) lib;
  evaluate =
    modules:
    (inputs.nixpkgs.lib.nixosSystem {
      system = pkgs.stdenv.hostPlatform.system;
      inherit modules;
      inherit (applicationCatalog) specialArgs;
    }).config;
  configurations = {
    live = installer.config;
    aizome = evaluate [
      inputs.aizome.nixosModules.default
      { programs.aizome.enable = true; }
    ];
    tatami = evaluate [
      inputs.tatami.nixosModules.default
      { programs.tatami.enable = true; }
    ];
    installed = evaluate [
      inputs.calamares.nixosModules.default
      {
        calamares.desktops = [
          "aizome"
          "tatami"
        ];
        calamares.applications = [ ];
        calamares.installUser = "demo";
      }
    ];
  };
  checked = lib.mapAttrs (
    _: config:
    let
      packages = lib.unique config.environment.systemPackages;
      names = map lib.getName packages;
      terminals = config.xdg.terminal-exec.settings;
      hotaru =
        if config.programs.aizome.enable or false then
          config.programs.aizome.package
        else
          inputs.aizome.packages.${pkgs.stdenv.hostPlatform.system}.hotaru;
    in
    assert !(builtins.elem "foot" names);
    assert builtins.elem hotaru packages;
    assert
      builtins.length (
        builtins.filter (
          p:
          builtins.elem (lib.getName p) [
            "aizome"
            "hotaru"
          ]
        ) packages
      ) == 1;
    assert
      !(config.programs.aizome.enable or false) || terminals.Aizome == [ "org.aizome.Hotaru.desktop" ];
    assert
      !(config.programs.tatami.enable or false) || terminals.Hyprland == [ "org.aizome.Hotaru.desktop" ];
    {
      terminal = builtins.unsafeDiscardStringContext hotaru.outPath;
      inherit terminals;
    }
  ) configurations;
in
pkgs.writeText "sekka-terminal-defaults.json" (builtins.toJSON checked)
