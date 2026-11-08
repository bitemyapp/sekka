{ pkgs }:
let
  inherit (pkgs) lib;
  defaults = pkgs.writeText "aizome-mimeapps.list" (import ./mime-defaults.nix { inherit lib; });
  entries = lib.fileset.toSource {
    root = ../apps;
    fileset = lib.fileset.unions [
      ../apps/hako/data/org.aizome.Hako.desktop
      ../apps/koyomi/data/org.aizome.Koyomi.desktop
      ../apps/shashin/data/org.aizome.Shashin.desktop
      ../apps/shiori/data/org.aizome.Shiori.desktop
      ../apps/sumi/data/org.aizome.Sumi.desktop
      ../apps/tansu/data/org.aizome.Tansu.desktop
      ../apps/utsushi/data/org.aizome.Utsushi.desktop
    ];
  };
in
pkgs.runCommand "aizome-mime-defaults-test" { nativeBuildInputs = [ pkgs.python3 ]; } ''
  python3 - ${defaults} ${entries} <<'PY'
  import configparser
  import pathlib
  import sys

  def read(path):
      config = configparser.ConfigParser(interpolation=None)
      config.optionxform = str
      config.read(path)
      return config

  defaults = dict(read(sys.argv[1])["Default Applications"])
  expected = {}
  for path in pathlib.Path(sys.argv[2]).rglob("*.desktop"):
      for mime in read(path)["Desktop Entry"]["MimeType"].split(";"):
          if mime:
              assert mime not in expected, f"Conflicting defaults for {mime}"
              expected[mime] = path.name + ";"
  assert defaults == expected, (defaults.keys() ^ expected.keys())
  assert defaults["video/x-ms-wmv"] == "org.aizome.Utsushi.desktop;"
  assert defaults["audio/x-ms-wma"] == "org.aizome.Utsushi.desktop;"
  assert defaults["application/yaml"] == "org.aizome.Sumi.desktop;"
  print(f"All {len(expected)} advertised MIME types have their declared default.")
  PY
  touch $out
''
