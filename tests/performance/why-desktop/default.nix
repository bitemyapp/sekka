# Tools for docs/performance/why-desktop.md: the timer-driven update probe.
#
#   nix build --impure -f tests/performance/why-desktop probe
#
# Built against the same Nixpkgs as the desktop comparison's VMs.
{
  repo ? toString ../../..,
}:
let
  desktops = import ../desktops { inherit repo; };
  inherit (desktops) pkgs;
in
{
  inherit pkgs;
  probe = pkgs.stdenv.mkDerivation {
    pname = "why-desktop-probe";
    version = "1";
    src = pkgs.lib.fileset.toSource {
      root = ./.;
      fileset = ./commit-probe.c;
    };
    nativeBuildInputs = [
      pkgs.pkg-config
      pkgs.wayland-scanner
    ];
    buildInputs = [ pkgs.wayland ];
    buildPhase = ''
      runHook preBuild
      protocols=${pkgs.wayland-protocols}/share/wayland-protocols/stable
      for p in xdg-shell/xdg-shell presentation-time/presentation-time; do
        name=$(basename $p)
        wayland-scanner client-header $protocols/$p.xml $name-client-protocol.h
        wayland-scanner private-code $protocols/$p.xml $name-protocol.c
      done
      cc -O2 -Wall -Wextra -o commit-probe commit-probe.c xdg-shell-protocol.c presentation-time-protocol.c \
        $(pkg-config --cflags --libs wayland-client)
      runHook postBuild
    '';
    installPhase = ''
      runHook preInstall
      install -Dm755 -t $out/bin commit-probe
      runHook postInstall
    '';
  };
}
