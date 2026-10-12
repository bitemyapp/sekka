# sekka.computer: static pages built by the Rust generator in site/ from the
# brand tokens, logos and screenshots in this repository, with the fonts it
# serves taken from this Nixpkgs. The output is the directory to upload.
{
  pkgs,
  revision,
  date,
}:
let
  inherit (pkgs) lib;
  generator = pkgs.rustPlatform.buildRustPackage {
    pname = "sekka-site";
    version = "0.1.0";
    src = lib.fileset.toSource {
      root = ../site;
      fileset = lib.fileset.unions [
        ../site/Cargo.toml
        ../site/Cargo.lock
        ../site/src
        ../site/style.css
      ];
    };
    cargoLock.lockFile = ../site/Cargo.lock;
    meta.mainProgram = "sekka-site";
  };
  # Only what the pages show: changing anything else leaves the site alone.
  content = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [
      ../docs/brand/tokens.json
      ../docs/brand/logos
      ../docs/brand/family
      ../docs/images/sekka
      ../apps/yukimi/yukimi/data/io.github.bitemyapp.Yukimi.svg
    ];
  };
  mono = pkgs.jetbrains-mono;
  sans = pkgs.atkinson-hyperlegible-next;
in
pkgs.runCommand "sekka-site" { } ''
  ${lib.getExe generator} \
    --repo ${content} \
    --out $out \
    --revision ${lib.escapeShellArg revision} \
    --date ${lib.escapeShellArg date} \
    --mono "${mono}/share/fonts/truetype/JetBrainsMono[wght].ttf" \
    --mono-license ${mono.src}/OFL.txt \
    --sans "${sans}/share/fonts/truetype/AtkinsonHyperlegibleNext[wght].ttf" \
    --sans-italic "${sans}/share/fonts/truetype/AtkinsonHyperlegibleNext-Italic[wght].ttf" \
    --sans-license ${sans.src}/OFL.txt \
    --cjk ${pkgs.noto-fonts-cjk-serif}/share/fonts/opentype/noto-cjk/NotoSerifCJK-VF.otf.ttc
''
