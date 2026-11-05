# Real evaluator/source identities, without a host daemon or network access.
{ pkgs }:
let
  source = pkgs.lib.fileset.toSource {
    root = ../.;
    fileset = pkgs.lib.fileset.unions [
      ../scripts/check-build-inputs.py
      ./bazel/without-build-files.nix
      ./scheduling/aizome-launch-probe.nix
      ../desktop/aizome/crates/aizome-comp-core/src/session.rs
    ];
  };
in
pkgs.runCommand "build-input-stability"
  {
    nativeBuildInputs = [ pkgs.python3 pkgs.nix ];
  }
  ''
    export HOME="$TMPDIR/home"
    mkdir -p "$HOME"
    export NIX_CONFIG="experimental-features = nix-command flakes
    build-users-group =
    "
    python3 ${source}/scripts/check-build-inputs.py \
      --repo ${source} --nixpkgs ${pkgs.path} --store "$TMPDIR/private-store"
    printf '%s\n' 'Nix build-input identity assertions passed' > "$out"
  ''
