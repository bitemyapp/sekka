# Check optional installation separately from the ISO's offline app cache.
{
  pkgs,
  applicationCatalog,
  installer,
  uta,
}:
let
  inherit (pkgs) lib;
  catalog = applicationCatalog.catalog;
  entry = lib.findFirst (app: app.id == "uta") null catalog;
  selected = (applicationCatalog.evaluate [ "uta" ]).config;
  empty = (applicationCatalog.evaluate [ ]).config;
  preloaded = (applicationCatalog.evaluate (map (app: app.id) applicationCatalog.preloaded)).config;
  # Inspect the JSON values without turning every mentioned package into a
  # dependency of the parser; the report below retains Uta's real reference.
  manifest =
    config:
    builtins.fromJSON (
      builtins.unsafeDiscardStringContext config.environment.etc."installer-applications.json".text
    );
  hasUta =
    config: lib.any (package: toString package == toString uta) config.environment.systemPackages;
  iso = installer.config;
in
assert entry != null && entry.source == "sekka" && !entry.preload && !(entry.unfree or false);
assert hasUta selected && (manifest selected).selected == [ "uta" ];
assert !hasUta empty && !hasUta preloaded && !hasUta iso;
assert !(builtins.elem "uta" (manifest preloaded).selected);
assert applicationCatalog.cache.drvPath == preloaded.system.build.installerApplications.drvPath;
assert builtins.elem (toString applicationCatalog.cache) (map toString iso.isoImage.storeContents);
assert
  !(lib.any (file: file.target == "/calamares/closures/application-uta.paths") iso.isoImage.contents);
assert uta.meta.license.spdxId == "GPL-3.0-only";
pkgs.writeText "uta-installation.json" (
  builtins.toJSON {
    catalog = entry;
    selected = (manifest selected).selected;
    preload = (manifest preloaded).selected;
    package = toString uta;
    license = uta.meta.license.spdxId;
    # A manifest/build check, not certification of an installed release ISO.
    isoPreloaded = false;
  }
)
