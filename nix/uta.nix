# Uta (bitemyapp/uta), the YouTube Music client, built by its own
# nix/package.nix. Uta depends on Aizome's widget kit by path; its flake takes
# the kit from Aizome's former repository, so Sekka gives it the kit and
# Sukumo, the toolkit the kit draws with, from desktop/aizome instead.
{
  pkgs,
  uta,
  fonts,
}:
let
  inherit (pkgs) lib;
  # The kit's crates and Sukumo's, with the workspace manifests they inherit
  # from: changes to the rest of Aizome don't rebuild Uta.
  aizome = lib.fileset.toSource {
    root = ../desktop/aizome;
    fileset = lib.fileset.unions [
      ../desktop/aizome/Cargo.toml
      ../desktop/aizome/crates/aizome-kit
      ../desktop/aizome/crates/aizome-theme
      ../desktop/aizome/crates/aizome-config
      ../desktop/aizome/crates/aizome-shell-proto
      ../desktop/aizome/sukumo/Cargo.toml
      ../desktop/aizome/sukumo/crates
    ];
  };
in
pkgs.callPackage "${uta}/nix/package.nix" {
  src = uta;
  enableRustyPipe = true;
  inherit aizome fonts;
  # Uta's package still lays a Sidestep checkout out beside the kit, as it
  # did before Sukumo moved into desktop/aizome. The kit no longer reads it.
  sidestep = pkgs.emptyDirectory;
  # The music service's Python, as Uta's flake.nix makes it.
  python = pkgs.python3.withPackages (ps: [
    ps.ytmusicapi
    ps.yt-dlp
    ps.secretstorage
    ps.cryptography
  ]);
}
