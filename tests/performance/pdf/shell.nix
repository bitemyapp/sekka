# Build and run environment for the PDF renderer benchmarks, from the nixpkgs
# the Sekka flake pins (flake.lock's root `nixpkgs` input), so the engines'
# versions are the ones recorded in docs/performance/pdf.md.
#
#   nix-shell tests/performance/pdf/shell.nix --run 'tests/performance/pdf/build.sh'
let
  lock = builtins.fromJSON (builtins.readFile ../../../flake.lock);
  node = lock.nodes.${lock.nodes.root.inputs.nixpkgs}.locked;
  nixpkgs = builtins.fetchTarball {
    url = node.url;
    sha256 = node.narHash;
  };
  pkgs = import nixpkgs { };
in
pkgs.mkShell {
  packages = with pkgs; [
    cargo
    rustc
    pkg-config
    (python3.withPackages (ps: [ ps.numpy ps.pillow ]))
    pdfium
    mupdf
    poppler
    cairo
    glib
    git
    xz
    zstd
    util-linux
    coreutils
  ];
  # pdfium has no .pc file: its headers are in the dev output.
  PDFIUM_INCLUDE = "${pkgs.pdfium.dev}/include/public";
  PDFIUM_LIB = "${pkgs.pdfium}/lib";
}
