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
  rev = uta.rev or "(not a Git revision)";
  package = pkgs.callPackage "${uta}/nix/package.nix" {
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
  };
  # With RustyPipe the build is GPL-3.0-only. push-cache publishes these
  # sources beside the binary: Uta's pinned source (its vendored, patched
  # RustyPipe included), the kit and Sukumo it was built with, and this
  # recipe. Uta's Cargo.lock names the crates.io sources by checksum.
  source = pkgs.linkFarm "aizome-uta-${package.version}-source" [
    {
      name = "uta";
      path = uta;
    }
    {
      name = "aizome";
      path = aizome;
    }
    {
      name = "sekka/nix/uta.nix";
      path = ./uta.nix;
    }
  ];
  # Directions to that source, next to the binary. They name no store path,
  # so installing Uta doesn't download its source too.
  note = pkgs.writeText "aizome-uta-SOURCE" ''
    Uta for YouTube Music, as Sekka builds it

    This build links RustyPipe and is distributed under GPL-3.0-only; see
    LICENSE-RustyPipe and LICENSE-EJS beside this file.

    Source: https://github.com/bitemyapp/uta at revision ${rev}
    (Nix: github:bitemyapp/uta/${rev}, ${uta.narHash or "no narHash"}),
    with its vendored and patched RustyPipe in vendor/rustypipe.

    Sekka (https://github.com/bitemyapp/sekka) builds it with Uta's own
    nix/package.nix, RustyPipe enabled, by Sekka's recipe nix/uta.nix: against
    Aizome's widget kit and Sukumo from Sekka's desktop/aizome, at the Sekka
    revision this system was built from. Uta's Cargo.lock names, with
    checksums, the crates.io sources it compiles.

    Sekka's binary cache, https://cache.sekka.computer, publishes those
    sources with each Uta build it serves; from that Sekka revision,
    `nix build .#uta.source` collects them.
  '';
in
package.overrideAttrs (old: {
  postInstall = (old.postInstall or "") + ''
    install -Dm644 ${note} $out/share/doc/aizome-uta/SOURCE
  '';
  passthru = (old.passthru or { }) // {
    inherit source;
  };
})
