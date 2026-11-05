# The probe client and key injector the benchmark runs inside each VM.
{
  lib,
  stdenv,
  pkg-config,
  wayland,
  wayland-scanner,
  wayland-protocols,
}:
stdenv.mkDerivation {
  pname = "desktop-perf-tools";
  version = "1";
  src = lib.fileset.toSource {
    root = ./.;
    fileset = lib.fileset.unions [
      ./wl-probe.c
      ./uinput-keys.c
      ./perf-util.c
    ];
  };
  nativeBuildInputs = [
    pkg-config
    wayland-scanner
  ];
  buildInputs = [ wayland ];
  buildPhase = ''
    runHook preBuild
    protocols=${wayland-protocols}/share/wayland-protocols/stable
    for p in xdg-shell/xdg-shell presentation-time/presentation-time; do
      name=$(basename $p)
      wayland-scanner client-header $protocols/$p.xml $name-client-protocol.h
      wayland-scanner private-code $protocols/$p.xml $name-protocol.c
    done
    cc -O2 -Wall -Wextra -Werror -o wl-probe wl-probe.c xdg-shell-protocol.c presentation-time-protocol.c \
      $(pkg-config --cflags --libs wayland-client)
    cc -O2 -Wall -Wextra -Werror -o uinput-keys uinput-keys.c
    cc -O2 -Wall -Wextra -Werror -pthread -o perf-util perf-util.c
    runHook postBuild
  '';
  installPhase = ''
    runHook preInstall
    install -Dm755 -t $out/bin wl-probe uinput-keys perf-util
    runHook postInstall
  '';
}
