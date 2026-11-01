# Keep the session defaults in step with the player's advertised formats.
{ lib }:
let
  lines = lib.splitString "\n" (builtins.readFile ../apps/utsushi/data/org.aizome.Utsushi.desktop);
  mime = lib.findFirst (lib.hasPrefix "MimeType=") (throw "Utsushi has no MimeType entry") lines;
in
lib.filter (type: type != "") (lib.splitString ";" (lib.removePrefix "MimeType=" mime))
