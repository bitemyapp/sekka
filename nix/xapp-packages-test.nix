{
  pkgs,
  inputs,
  applicationCatalog,
  patched,
  compatibility,
}:
let
  references = import ./reference-systems.nix {
    inherit inputs applicationCatalog;
    desktops = [ "xfce" ];
  };
  installed = inputs.nixpkgs.lib.nixosSystem {
    system = pkgs.stdenv.hostPlatform.system;
    inherit (applicationCatalog) specialArgs;
    modules = [
      inputs.calamares.nixosModules.default
      { calamares.desktops = [ "xfce" ]; }
    ];
  };
  configurations = {
    installed = installed.config;
    iso-reference = references.systems.xfce.config;
    live-compatibility = compatibility;
  };
  paths = pkgs.lib.mapAttrs (
    name: config:
    assert config.services.xserver.desktopManager.xfce.enable;
    assert builtins.elem patched.outPath (map (p: p.outPath) config.xdg.portal.extraPortals);
    patched.outPath
  ) configurations;
in
pkgs.writeText "sekka-xapp-packages.json" (builtins.toJSON paths)
