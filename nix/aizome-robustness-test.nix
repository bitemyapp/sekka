# Headless compositor and shell regressions from the same filtered source
# and locked dependencies as the desktop package. No host session is used.
{
  pkgs,
  aizome,
  fonts,
}:
aizome.overrideAttrs (_old: {
  pname = "aizome-robustness-tests";
  outputs = [ "out" ];
  buildPhase = ''
    runHook preBuild
    export RUSTC_WRAPPER=$PWD/scripts/rustc-wrapper
    export CARGO_NET_OFFLINE=true
    export CARGO_BUILD_JOBS=$NIX_BUILD_CORES
    runHook postBuild
  '';
  checkPhase = ''
    runHook preCheck
    export AIZOME_FONT_DIR=${fonts}
    export LD_LIBRARY_PATH=${
      pkgs.lib.makeLibraryPath (
        with pkgs;
        [
          libglvnd
          libpulseaudio
          fontconfig
          libxkbcommon
          wayland
        ]
      )
    }
    export FONTCONFIG_FILE=${
      pkgs.makeFontsConf {
        fontDirectories = [
          fonts
          pkgs.noto-fonts-cjk-sans
        ];
      }
    }
    export XKB_CONFIG_ROOT=${pkgs.xkeyboard_config}/share/X11/xkb
    export TZDIR=${pkgs.tzdata}/share/zoneinfo
    export XDG_CACHE_HOME=$TMPDIR/xdg-cache
    mkdir -p "$XDG_CACHE_HOME"
    unset WAYLAND_DISPLAY WAYLAND_SOCKET DISPLAY
    cargo test --offline --locked \
      -p aizome-comp -p aizome-comp-core -p aizome-comp-logic -p aizome-shell -p aizome-portal \
      --lib --test service --test churn --test recovery --test reload --test lock_displays --test protocols --test pacing
    runHook postCheck
  '';
  installPhase = ''
    mkdir -p $out
    echo "Aizome component robustness regressions passed" > $out/result
  '';
  postFixup = "";
})
