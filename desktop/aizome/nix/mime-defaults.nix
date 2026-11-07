# The desktop entries are the authoritative supported-format declarations.
# This list chooses the default apps; adding a supported type to an entry
# also gives it the same session default, without another format table.
{ lib }:
let
  apps = {
    Hako = ../apps/hako/data/org.aizome.Hako.desktop;
    Koyomi = ../apps/koyomi/data/org.aizome.Koyomi.desktop;
    Shashin = ../apps/shashin/data/org.aizome.Shashin.desktop;
    Shiori = ../apps/shiori/data/org.aizome.Shiori.desktop;
    Sumi = ../apps/sumi/data/org.aizome.Sumi.desktop;
    Tansu = ../apps/tansu/data/org.aizome.Tansu.desktop;
    Utsushi = ../apps/utsushi/data/org.aizome.Utsushi.desktop;
  };
  types =
    file:
    let
      lines = lib.filter (lib.hasPrefix "MimeType=") (lib.splitString "\n" (builtins.readFile file));
    in
    assert lib.assertMsg (builtins.length lines == 1) "Expected one MimeType declaration in ${file}";
    lib.unique (
      lib.filter (s: s != "") (lib.splitString ";" (lib.removePrefix "MimeType=" (builtins.head lines)))
    );
in
"[Default Applications]\n"
+ lib.concatStrings (
  lib.mapAttrsToList (
    app: file: lib.concatMapStrings (mime: "${mime}=org.aizome.${app}.desktop;\n") (types file)
  ) apps
)
