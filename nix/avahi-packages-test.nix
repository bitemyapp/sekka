# Check the daemon selected by live boots, installed desktop choices and the
# prebuilt systems that Genkan copies from the media.
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
pkgs.writeText "sekka-avahi-packages.json" (
  builtins.toJSON (
    lib.mapAttrs (
      _: config:
      assert config.services.avahi.package.outPath == patched.outPath;
      {
        inherit (config.services.avahi) enable;
        package = config.services.avahi.package.outPath;
      }
    ) configurations
  )
)
