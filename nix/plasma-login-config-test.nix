# Compile the actual frontend config loader from the selected package's patched
# source, with disposable config paths. No GUI or ISO qualification claim.
{ pkgs, desktopsModule }:
let
  installed = import (pkgs.path + "/nixos/lib/eval-config.nix") {
    system = pkgs.stdenv.hostPlatform.system;
    modules = [
      desktopsModule
      {
        calamares.desktops = [
          "xfce"
          "plasma"
        ];
        calamares.defaultDesktop = "xfce";
      }
    ];
  };
  package = installed.config.services.displayManager.plasma-login-manager.package;
  source =
    name: selected:
    pkgs.stdenvNoCC.mkDerivation {
      inherit name;
      inherit (selected) src patches;
      phases = [
        "unpackPhase"
        "patchPhase"
        "installPhase"
      ];
      installPhase = ''
        mkdir -p "$out"
        cp src/frontend/settings/plasmaloginsettings.cpp "$out/loader.cpp"
      '';
    };
  original = source "plasmalogin-original-loader" pkgs.kdePackages.plasma-login-manager;
  fixed = source "plasmalogin-fixed-loader" package;
in
pkgs.runCommand "sekka-plasma-login-config"
  {
    nativeBuildInputs = [
      pkgs.stdenv.cc
      pkgs.pkg-config
      pkgs.python3
    ];
    buildInputs = [
      pkgs.kdePackages.kconfig
      pkgs.qt6.qtbase
    ];
  }
  ''
    export HOME=$TMPDIR/home
    export LC_ALL=C.UTF-8
    mkdir -p "$HOME" "$out"
    for variant in original fixed; do
      if [ "$variant" = original ]; then source=${original}; else source=${fixed}; fi
      mkdir -p "$out/$variant"
      python3 ${../tests/plasma-login-config.py} "$source/loader.cpp" "$TMPDIR/$variant" > "$out/$variant/loader-test.cpp"
      c++ "$out/$variant/loader-test.cpp" -o "$out/$variant/loader-test" \
        -I${pkgs.kdePackages.kconfig.dev}/include/KF6/KConfigCore \
        -I${pkgs.kdePackages.kconfig.dev}/include/KF6/KConfig \
        -lKF6ConfigCore $(pkg-config --cflags --libs Qt6Core)
      "$out/$variant/loader-test" "$variant" | tee "$out/$variant/results.txt"
    done
  ''
