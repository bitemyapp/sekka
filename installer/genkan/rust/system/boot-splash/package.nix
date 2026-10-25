# Sekka's Plymouth theme: sekka.script, the stacked lockup from docs/brand
# and the pieces generate.py draws, each rasterized at 1x, 2x and 3x.
{
  lib,
  stdenvNoCC,
  python3,
  resvg,
  ibm-plex,
}:
let
  root = ../../../../..;
  here = "installer/genkan/rust/system/boot-splash";
in
stdenvNoCC.mkDerivation {
  pname = "sekka-plymouth-theme";
  version = "1";
  src = lib.fileset.toSource {
    inherit root;
    fileset = lib.fileset.unions [
      ./generate.py
      ./sekka.script
      (root + "/docs/brand/logos/sekka-stacked-night.svg")
    ];
  };
  nativeBuildInputs = [
    (python3.withPackages (ps: [ ps.fonttools ]))
    resvg
  ];
  buildPhase = ''
    runHook preBuild
    python3 ${here}/generate.py svg \
      ${ibm-plex}/share/fonts/truetype/IBMPlexMono-Regular.ttf \
      ${ibm-plex}/share/fonts/truetype/IBMPlexMono-Medium.ttf
    cp docs/brand/logos/sekka-stacked-night.svg svg/lockup.svg
    mkdir png
    for svg in svg/*.svg; do
      for scale in 1 2 3; do
        resvg --zoom $scale "$svg" "png/$(basename "$svg" .svg)-''${scale}x.png"
      done
    done
    runHook postBuild
  '';
  installPhase = ''
    runHook preInstall
    theme=$out/share/plymouth/themes/sekka
    mkdir -p $theme
    cp png/*.png ${here}/sekka.script $theme/
    cat > $theme/sekka.plymouth <<EOF
    [Plymouth Theme]
    Name=Sekka
    Description=Sekka's crest on the Night ground, with a rail that fills as the computer starts
    ModuleName=script

    [script]
    ImageDir=$theme
    ScriptFile=$theme/sekka.script
    EOF
    runHook postInstall
  '';
  meta = {
    description = "Sekka's boot splash for Plymouth";
    license = lib.licenses.gpl3Plus;
    platforms = lib.platforms.linux;
  };
}
