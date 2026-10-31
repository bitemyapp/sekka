# aizome.computer: static pages built by the Rust generator in aizome-site/
# from Aizome's theme, icons and screenshots in this repository, with the
# fonts it serves taken from Aizome's own Archivo and this Nixpkgs. The
# output is the directory to deploy, with Wrangler (DEVELOPERS.md).
{
  pkgs,
  revision,
  date,
}:
let
  inherit (pkgs) lib;
  generator = pkgs.rustPlatform.buildRustPackage {
    pname = "aizome-site";
    version = "0.1.0";
    src = lib.fileset.toSource {
      root = ../aizome-site;
      fileset = lib.fileset.unions [
        ../aizome-site/Cargo.toml
        ../aizome-site/Cargo.lock
        ../aizome-site/src
        ../aizome-site/style.css
        ../aizome-site/theme.js
      ];
    };
    cargoLock.lockFile = ../aizome-site/Cargo.lock;
    meta.mainProgram = "aizome-site";
  };

  # Only what the pages show: changing anything else leaves the site alone.
  # The screenshots are listed one by one, as the generator's IMAGES lists
  # them; the theme crate's source is where the site's colours come from.
  content = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [
      ../desktop/aizome/crates/aizome-theme/src/lib.rs
      ../desktop/aizome/icons/apps
      ../desktop/aizome/apps/hotaru/data/org.aizome.Hotaru.svg
      ../docs/images/sekka/aizome-desktop.png
      ../docs/images/sekka/aizome-menu.png
      ../docs/images/sekka/aizome-palette.png
      ../docs/images/sekka/aizome-settings.png
      ../docs/images/sekka/aizome-yukimi.png
      ../docs/images/sekka/genkan.png
      ../desktop/aizome/docs/screenshots/01-desktop-night.png
      ../desktop/aizome/docs/screenshots/02-menus.png
      ../desktop/aizome/docs/screenshots/03-palette.png
      ../desktop/aizome/docs/screenshots/04-overview.png
      ../desktop/aizome/docs/screenshots/05-switcher.png
      ../desktop/aizome/docs/screenshots/07-day.png
      ../desktop/aizome/docs/screenshots/09-kura.png
      ../desktop/aizome/docs/screenshots/10-tokei-tenki.png
      ../desktop/aizome/docs/screenshots/11-marking-menu.png
      ../desktop/aizome/docs/screenshots/12-lock.png
      ../desktop/aizome/docs/screenshots/13-full-effects.png
      ../desktop/aizome/docs/screenshots/14-recording.png
      ../desktop/aizome/docs/screenshots/16-recording-saved.png
      ../desktop/aizome/docs/screenshots/recovery.png
      ../desktop/aizome/docs/screenshots/nisshi/night-selected.png
      ../desktop/aizome/docs/screenshots/nisshi/day-search.png
      ../desktop/aizome/docs/images/heterogeneous-group-calculator.png
      ../desktop/aizome/docs/images/heterogeneous-group-browser.png
      ../desktop/aizome/docs/images/heterogeneous-group-detached.png
      ../docs/test-results/application-startup/koe-window-live.png
      ../docs/test-results/luks-gui-uefi/75-settings-displays.png
    ];
  };

  # Aizome's interface face, the file its session installs, served as WOFF2:
  # the same font, compressed for the web.
  archivo = pkgs.callPackage ../desktop/aizome/nix/fonts.nix { };
  archivoWoff2 = pkgs.runCommand "archivo-woff2" { nativeBuildInputs = [ pkgs.woff2 ]; } ''
    cp ${archivo}/share/fonts/truetype/Archivo-VF.ttf Archivo.ttf
    woff2_compress Archivo.ttf
    install -Dm444 Archivo.woff2 $out/Archivo.woff2
  '';
  # IBM Plex Mono as IBM publishes it for the web.
  plex = pkgs.ibm-plex.mono;
  noto = "${pkgs.noto-fonts}/share/fonts/noto";
in
pkgs.runCommand "aizome-site" { } ''
  ${lib.getExe generator} \
    --repo ${content} \
    --out $out \
    --revision ${lib.escapeShellArg revision} \
    --date ${lib.escapeShellArg date} \
    --sans ${archivoWoff2}/Archivo.woff2 \
    --sans-license ${archivo}/share/licenses/archivo/OFL.txt \
    --mono ${plex.webfont}/share/fonts/woff2/IBMPlexMono-Regular.woff2 \
    --mono-semibold ${plex.webfont}/share/fonts/woff2/IBMPlexMono-SemiBold.woff2 \
    --mono-license ${plex.src}/LICENSE.txt \
    --cjk ${pkgs.noto-fonts-cjk-serif}/share/fonts/opentype/noto-cjk/NotoSerifCJK-VF.otf.ttc \
    --symbols ${noto}/NotoSansSymbols2-Regular.otf:${noto}/NotoSansSymbols.ttf:${noto}/NotoSansMath-Regular.otf
''
