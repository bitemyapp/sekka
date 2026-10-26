# Aizome's typefaces, all under the SIL Open Font License:
#   Archivo (variable: width 62-125, weight 100-900), from Google Fonts;
#   IBM Plex Mono, Noto Sans Symbols 2 and Noto Serif CJK come from nixpkgs
#   (see module.nix, which adds them to fonts.packages).
{ lib, stdenvNoCC, fetchurl }:
let
  rev = "95f4904fc8bcf26d3420fe315560c96417c6dec7";
  base = "https://raw.githubusercontent.com/google/fonts/${rev}/ofl/archivo";
in
stdenvNoCC.mkDerivation {
  pname = "archivo";
  version = "2.001-unstable-${builtins.substring 0 7 rev}";
  srcs = [
    (fetchurl {
      name = "archivo-vf.ttf";
      url = "${base}/Archivo%5Bwdth,wght%5D.ttf";
      hash = "sha256-DglKfTx8TCXPExDEswAU8drpMyIgscLIj0+plvCwUFM=";
    })
    (fetchurl {
      name = "archivo-italic-vf.ttf";
      url = "${base}/Archivo-Italic%5Bwdth,wght%5D.ttf";
      hash = "sha256-MF0TtOuA5i99UX9418BFJQ2XdVKkIPLb7ZBvMU52EwU=";
    })
    (fetchurl {
      name = "archivo-OFL.txt";
      url = "${base}/OFL.txt";
      hash = "sha256-EItOV8nHltPTjQQoyn7jneR62TGHMCcY2bLYhkubcWs=";
    })
  ];
  dontUnpack = true;
  installPhase = ''
    runHook preInstall
    set -- $srcs
    install -Dm644 $1 $out/share/fonts/truetype/Archivo-VF.ttf
    install -Dm644 $2 $out/share/fonts/truetype/Archivo-Italic-VF.ttf
    install -Dm644 $3 $out/share/licenses/archivo/OFL.txt
    runHook postInstall
  '';
  meta = {
    description = "Archivo, Aizome's interface typeface (variable width and weight)";
    homepage = "https://github.com/Omnibus-Type/Archivo";
    license = lib.licenses.ofl;
    platforms = lib.platforms.all;
  };
}
