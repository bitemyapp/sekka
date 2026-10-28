# The font files Aizome draws with, in one folder, as the kit and the
# compositor look for them in $AIZOME_FONT_DIR: the same files the session
# installs (module.nix's fonts.packages), so tests, goldens and a dev run
# set the type the session shows. `nix build .#aizome-fonts -o ~/work/.aizome-fonts`
# makes or refreshes the folder (docs/kit.md).
{
  lib,
  runCommand,
  archivo,
  ibm-plex,
  noto-fonts,
}:
runCommand "aizome-fonts"
  {
    meta = {
      description = "Archivo, IBM Plex Mono and Noto Sans Symbols 2, as Aizome's session installs them";
      license = lib.licenses.ofl;
      platforms = lib.platforms.all;
    };
  }
  ''
    mkdir -p $out
    # Copy these unmodified faces: symlinks would retain all of IBM Plex's
    # CJK families and Noto's other fonts in a standalone app's closure.
    face() { install -m444 "$1" $out/; }
    face ${archivo}/share/fonts/truetype/Archivo-VF.ttf
    face ${archivo}/share/fonts/truetype/Archivo-Italic-VF.ttf
    for face in Regular Medium SemiBold Bold Italic; do
      install -m444 ${ibm-plex}/share/fonts/truetype/IBMPlexMono-$face.ttf $out/
    done
    face ${noto-fonts}/share/fonts/noto/NotoSansSymbols2-Regular.otf
    install -m444 ${archivo}/share/licenses/archivo/OFL.txt $out/OFL-Archivo.txt
  ''
