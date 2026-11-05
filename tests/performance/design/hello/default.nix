# The hello-window programs for GTK 4, libadwaita, Qt 6 Widgets and Qt 6
# Quick, from Sekka's Nixpkgs. Built from the repository root with
#   nix build --impure --expr 'let f = builtins.getFlake (toString ./.); pkgs = f.inputs.nixpkgs.legacyPackages.x86_64-linux; in pkgs.callPackage ./tests/performance/design/hello { }'
# The kit's program (kit/) is built with Cargo in Aizome's development shell.
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
    name: deps: modules:
    stdenv.mkDerivation {
      pname = "hello-${name}";
      version = "1";
      src = ./${name};
      nativeBuildInputs = [
        pkg-config
        wrapGAppsHook4
      ];
      buildInputs = deps;
      buildPhase = ''
        $CC -O2 -o hello-${name} hello.c $(pkg-config --cflags --libs ${lib.concatStringsSep " " modules})
      '';
      installPhase = "install -Dm755 hello-${name} $out/bin/hello-${name}";
    };
  qtProgram =
    name: src: modules:
    stdenv.mkDerivation {
      pname = "hello-${name}";
      version = "1";
      src = ./${name};
      nativeBuildInputs = [
        pkg-config
        qt6.wrapQtAppsHook
      ];
      buildInputs = [ qt6.qtwayland ] ++ map (m: qt6.${m}) [ "qtbase" "qtdeclarative" ];
      buildPhase = ''
        $CXX -O2 -std=c++17 -fPIC -o hello-${name} ${src} $(pkg-config --cflags --libs ${lib.concatStringsSep " " modules})
      '';
      installPhase = "install -Dm755 hello-${name} $out/bin/hello-${name}";
    };
in
symlinkJoin {
  name = "hello-windows";
  paths = [
    (gtkProgram "gtk4" [ gtk4 ] [ "gtk4" ])
    (gtkProgram "adw" [ gtk4 libadwaita ] [ "libadwaita-1" ])
    (qtProgram "qtwidgets" "hello.cpp" [ "Qt6Widgets" ])
    (qtProgram "qtquick" "main.cpp" [ "Qt6Quick" "Qt6Qml" "Qt6Gui" ])
  ];
}
