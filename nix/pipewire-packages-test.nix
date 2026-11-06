# Check the service package and unchanged client libraries for each desktop.
{
  pkgs,
  installer,
  references,
  installed,
  patched,
}:
let
  inherit (pkgs) lib;
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
