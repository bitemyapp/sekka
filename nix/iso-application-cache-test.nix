# The media carries the offline application cache, and Uta, an optional
# application, stays out of it (uta-installation checks the catalog side).
{
  pkgs,
  applicationCatalog,
  installer,
}:
let
  inherit (pkgs) lib;
  iso = installer.config;
in
assert builtins.elem (toString applicationCatalog.cache) (map toString iso.isoImage.storeContents);
assert
  !(lib.any (file: file.target == "/calamares/closures/application-uta.paths") iso.isoImage.contents);
pkgs.writeText "iso-application-cache.json" (
  builtins.toJSON {
    applicationCache = true;
    utaPreloaded = false;
  }
)
