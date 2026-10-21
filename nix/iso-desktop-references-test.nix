# Compare the actual GUI inventory with the evaluated ISO's prebuilt systems.
{
  pkgs,
  inputs,
  applicationCatalog,
  fixture,
  installer,
}:
let
  inherit (pkgs) lib;
  config = installer.config;
  desktops = config.respin.referenceDesktops;
  references = import ./reference-systems.nix {
    inherit inputs applicationCatalog desktops;
  };
  roots = map toString config.isoImage.storeContents;
  files = map (entry: entry.target) config.isoImage.contents;
in
# A name in referenceDesktops alone is not enough: both the system closure and
# the installer's precache list must actually be included on the ISO.
assert lib.all (system: builtins.elem (toString system) roots) references.toplevels;
assert lib.all (
  desktop: builtins.elem "/calamares/closures/desktop-${desktop}.paths" files
) desktops;
pkgs.runCommand "sekka-iso-desktop-references"
  {
    nativeBuildInputs = [ pkgs.python3 ];
    referenceDesktops = builtins.toJSON desktops;
    passAsFile = [ "referenceDesktops" ];
  }
  ''
    ${fixture}/bin/calamares-vm-fixture desktop-choices > offered.json
    python3 - "$referenceDesktopsPath" offered.json "$out" <<'PY'
    import json, pathlib, sys
    references = json.loads(pathlib.Path(sys.argv[1]).read_text())
    offered = json.loads(pathlib.Path(sys.argv[2]).read_text())
    assert len(references) == len(set(references)), references
    assert set(references) == set(offered), {
        "missing": sorted(set(offered) - set(references)),
        "unexpected": sorted(set(references) - set(offered)),
    }
    pathlib.Path(sys.argv[3]).write_text(json.dumps({"desktops": offered}) + "\n")
    PY
  ''
