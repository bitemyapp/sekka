# What libretro-database knows of the games Chanoma plays (the year, the
# genre, how many can play, who made them), for its shelves and settings:
# RetroArch's databases (`rdb/`, CC BY-SA 4.0) for the systems Chanoma
# has cores for, unchanged, with their licence. The whole repository is
# close to a gigabyte, so only these files are fetched, at one commit.
# metadata.rs reads them; module.nix's programs.aizome.chanoma.metadata
# points Chanoma at them.
{
  lib,
  stdenvNoCC,
  fetchurl,
}:
let
  rev = "2fdd069d45883449cecf97deddd493492e77ff29";
  raw = path: "https://raw.githubusercontent.com/libretro/libretro-database/${rev}/${path}";
  databases = {
    "Atari - 2600" = "sha256-bHuJ3S6jTIy31DgeLcsWrAGapGawreVQy8s8pvERVKU=";
    "Atari - Lynx" = "sha256-x9lRwiL7ZlJGzBLaieBQLDFjVPimxYj4n9/Ol63uryA=";
    "Bandai - WonderSwan" = "sha256-eseb+ZZMR1X2i+P+Jv4hinPltK87WoIbRU5tIXCCA/s=";
    "Bandai - WonderSwan Color" = "sha256-Kr0SkSSP7nlg614A/TPJxOtXraPU6hw5CHDJeSNChMI=";
    "NEC - PC Engine - TurboGrafx 16" = "sha256-i8dPOyyODGCKjinNALmT7xhAaVRpHW4zpFkdVvs8sIo=";
    "NEC - PC Engine SuperGrafx" = "sha256-TOOAxI5qTLjWKfJQ4scnOGBizkqkrhGTBt0zpgyPBCo=";
    "Nintendo - Game Boy" = "sha256-9X8xY3MkdwdxNanMwb9T97MG25ZA1OmN0tzjXc88+h4=";
    "Nintendo - Game Boy Advance" = "sha256-Brb1JcYryZ9ieiV51i4DH6WerYvE81qSPNZnxc33KkE=";
    "Nintendo - Game Boy Color" = "sha256-0nyzRoGZPLBX2XEhwvDjHp82xkc49axJpHwGaUMtjo4=";
    "Nintendo - Nintendo 64" = "sha256-weERva9BtnGsTXZrvhg2TixFXcRtk7JH/lHlhXnR/SU=";
    "Nintendo - Nintendo Entertainment System" = "sha256-UKXX5+n6jcOa+v6Qf0ZxuimIxLvMBpg0/umvLgwU/+E=";
    "Nintendo - Super Nintendo Entertainment System" = "sha256-QGiWHB36p+BZlfJepD53f0iydERGF44J/QYEDoRq4M4=";
    "SNK - Neo Geo Pocket" = "sha256-t2TXSAbEFEbiKd10Agzs8LP0vnt8hazcRjGVfSOG5vQ=";
    "SNK - Neo Geo Pocket Color" = "sha256-bCpAoglWRchRWChxayU5rquzfe654eVUB8xjFNyYEBg=";
    "Sega - Dreamcast" = "sha256-IgSHHIGTFAK+m32U+k8NhJJkqenbpADDyf0BiUR3rQg=";
    "Sega - Game Gear" = "sha256-0dR0aITs/mO5BpEnD94M+KP57Js+gH5ZDaLPyviQJ4Q=";
    "Sega - Master System - Mark III" = "sha256-4HZpB157K7IuEUPQCQ5txVTes6sHJpYGge5jaclM48E=";
    "Sega - Mega Drive - Genesis" = "sha256-5bpANlF2tLhJ+mZ6MrDksX4axvSQoIMjWHLBzvLwzFc=";
    "Sega - Saturn" = "sha256-HnMLgUA9xfX9LUn6s2ulBIAcUV/ZWdKXPE6pexu6vAY=";
    "Sony - PlayStation" = "sha256-mYyNsBN7w2hGkeUgQyOqfwFedRIHlIdL+2h8p3075qs=";
    "Sony - PlayStation 2" = "sha256-DI8JQcQTHiniiNs9b2EQFu/96jAw9YoDdld9i+kOINo=";
  };
  # Store paths can't hold spaces: each file is fetched under a name of
  # its own and installed under libretro's.
  fetch =
    name: hash:
    fetchurl {
      name = "libretro-database-" + lib.replaceStrings [ " " ] [ "_" ] name + ".rdb";
      url = raw "rdb/${lib.escapeURL name}.rdb";
      inherit hash;
    };
  license = fetchurl {
    name = "libretro-database-LICENSE";
    url = raw "LICENSE";
    hash = "sha256-I+54yLrknPCOovDISUXGa5h+vkUgiB+1Gz2tT7Q9B8I=";
  };
in
stdenvNoCC.mkDerivation {
  pname = "chanoma-metadata";
  version = "0-unstable-2026-11-01";

  dontUnpack = true;

  installPhase = ''
    runHook preInstall
    dir=$out/share/chanoma/metadata
    mkdir -p "$dir"
    ${lib.concatStrings (
      lib.mapAttrsToList (name: hash: ''
        cp ${fetch name hash} "$dir/${name}.rdb"
      '') databases
    )}
    cp ${license} "$dir/LICENSE"
    cat > "$dir/README" <<EOF
    RetroArch's game databases from libretro-database
    (https://github.com/libretro/libretro-database), commit ${rev},
    unchanged. Licensed CC BY-SA 4.0 (LICENSE), by the libretro project's
    contributors.
    EOF
    runHook postInstall
  '';

  meta = {
    description = "libretro-database's game databases for Chanoma's systems";
    homepage = "https://github.com/libretro/libretro-database";
    license = lib.licenses.cc-by-sa-40;
    platforms = lib.platforms.all;
  };
}
