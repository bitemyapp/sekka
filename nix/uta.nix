# Uta (bitemyapp/uta), the YouTube Music client, built by its own
# nix/package.nix. Uta depends on Aizome's widget kit by path; its flake takes
# the kit from Aizome's former repository, so Sekka gives it the kit from
# desktop/aizome and Sidestep from Sekka's `sidestep` input instead.
{
  pkgs,
  uta,
  sidestep,
  fonts,
}:
let
  inherit (pkgs) lib;
  # The kit's crates and the workspace manifest they inherit from: changes
  # to the rest of Aizome don't rebuild Uta.
  aizome = lib.fileset.toSource {
    root = ../desktop/aizome;
    fileset = lib.fileset.unions [
      ../desktop/aizome/Cargo.toml
      ../desktop/aizome/crates/aizome-kit
      ../desktop/aizome/crates/aizome-theme
      ../desktop/aizome/crates/aizome-config
      ../desktop/aizome/crates/aizome-shell-proto
    ];
  };
in
pkgs.callPackage "${uta}/nix/package.nix" {
  src = uta;
  inherit aizome sidestep fonts;
  # The music service's Python, as Uta's flake.nix makes it.
  python = pkgs.python3.withPackages (ps: [
    ps.ytmusicapi
    ps.yt-dlp
    ps.secretstorage
    ps.cryptography
  ]);
}
