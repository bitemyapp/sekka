# Check the service package and unchanged client libraries for each desktop.
{
  pkgs,
  inputs,
  installer,
  applicationCatalog,
  patched,
}:
let
  inherit (pkgs) lib;
  references = import ./reference-systems.nix {
    inherit inputs applicationCatalog;
    desktops = installer.config.respin.referenceDesktops;
  };
  installed = inputs.nixpkgs.lib.nixosSystem {
    system = pkgs.stdenv.hostPlatform.system;
    inherit (applicationCatalog) specialArgs;
    modules = [ inputs.calamares.nixosModules.default ];
  };
  configurations = {
    installed-default = installed.config;
    live-default = installer.config;
    iso-together = references.together.config;
  }
  // lib.mapAttrs' (
    name: s: lib.nameValuePair "live-${name}" s.configuration
  ) installer.config.specialisation
  // lib.mapAttrs' (name: s: lib.nameValuePair "iso-${name}" s.config) references.systems;
in
assert installed.pkgs.pipewire.outPath == pkgs.pipewire.outPath;
assert installer.pkgs.pipewire.outPath == pkgs.pipewire.outPath;
assert lib.all (s: s.pkgs.pipewire.outPath == pkgs.pipewire.outPath) (
  [ references.together ] ++ builtins.attrValues references.systems
);
pkgs.writeText "sekka-pipewire-packages.json" (
  builtins.toJSON (
    lib.mapAttrs (
      _: config:
      assert config.services.pipewire.package.outPath == patched.outPath;
      {
        inherit (config.services.pipewire) enable;
        package = config.services.pipewire.package.outPath;
      }
    ) configurations
  )
)
