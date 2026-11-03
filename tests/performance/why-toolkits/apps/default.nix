# The matched programs for GTK 4, libadwaita, Qt 6 Widgets and Qt 6 Quick
# (docs/performance/why-toolkits.md), from Sekka's Nixpkgs. Built from the
# repository root with
#   nix build --impure --expr 'let f = builtins.getFlake (toString ./.); pkgs = f.inputs.nixpkgs.legacyPackages.x86_64-linux; in pkgs.callPackage ./tests/performance/why-toolkits/apps { }'
# The kit's program (kit/) is built with Cargo in Aizome's development shell.
# Built -O2 with frame pointers, so perf can walk the programs' own stacks.
{
  lib,
  stdenv,
  pkg-config,
  wrapGAppsHook4,
  gtk4,
  libadwaita,
  qt6,
  symlinkJoin,
}:
let
  gtkProgram =
    name: flags: deps: modules:
    stdenv.mkDerivation {
      pname = "tk-${name}";
      version = "1";
      src = lib.fileset.toSource {
        root = ./.;
        fileset = ./tk-gtk.c;
      };
      nativeBuildInputs = [
        pkg-config
        wrapGAppsHook4
      ];
      buildInputs = deps;
      buildPhase = ''
        $CC -O2 -fno-omit-frame-pointer ${flags} -o tk-${name} tk-gtk.c $(pkg-config --cflags --libs ${lib.concatStringsSep " " modules})
      '';
      installPhase = "install -Dm755 tk-${name} $out/bin/tk-${name}";
    };
  qtProgram =
    name: src: modules:
    stdenv.mkDerivation {
      pname = "tk-${name}";
      version = "1";
      src = lib.fileset.toSource {
        root = ./.;
        fileset = ./${src};
      };
      nativeBuildInputs = [
        pkg-config
        qt6.wrapQtAppsHook
      ];
      buildInputs = [
        qt6.qtwayland
        qt6.qtbase
        qt6.qtdeclarative
      ];
      buildPhase = ''
        $CXX -O2 -fno-omit-frame-pointer -std=c++17 -fPIC -o tk-${name} ${src} $(pkg-config --cflags --libs ${lib.concatStringsSep " " modules})
      '';
      installPhase = "install -Dm755 tk-${name} $out/bin/tk-${name}";
    };
in
symlinkJoin {
  name = "why-toolkits-apps";
  paths = [
    (gtkProgram "gtk4" "" [ gtk4 ] [ "gtk4" ])
    (gtkProgram "adw" "-DADW" [
      gtk4
      libadwaita
    ] [ "libadwaita-1" ])
    (qtProgram "qtwidgets" "tk-qtwidgets.cpp" [ "Qt6Widgets" ])
    (qtProgram "qtquick" "tk-qtquick.cpp" [
      "Qt6Quick"
      "Qt6Qml"
      "Qt6Gui"
    ])
  ];
}
