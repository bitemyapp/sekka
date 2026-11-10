# `nix run .#push-cache [FLAKE]`: uploads Sekka's own packages to the public
# binary cache at https://cache.sekka.computer (Cloudflare R2 bucket
# sekka-cache), leaving the rest of their closures to cache.nixos.org.
# DEVELOPERS.md describes the key and credentials it expects. The cache
# workflow runs it for every push to main, the nightly workflow for every
# revision it publishes.
{ pkgs }:
let
  preparation = pkgs.runCommand "sekka-cache-preparation" { } ''
    mkdir -p $out
    cp ${../scripts/prepare_cache.py} $out/prepare_cache.py
    cp ${../scripts/check_cache_coverage.py} $out/check_cache_coverage.py
  '';
in
pkgs.writeShellApplication {
  name = "sekka-push-cache";
  # Building and signing use the caller's own Nix and daemon.
  runtimeInputs = [
    pkgs.awscli2
    pkgs.python3
    pkgs.zstd
  ];
  text = ''
    flake=''${1:-.}
    config=''${SEKKA_CACHE_CONFIG:-''${XDG_CONFIG_HOME:-$HOME/.config}/sekka-cache}
    key=$config/cache.sekka.computer-1.sec
    endpoint=''${SEKKA_CACHE_ENDPOINT:-https://c408b91e58b3aa56c4f71046fcd416b6.r2.cloudflarestorage.com}
    bucket=''${SEKKA_CACHE_BUCKET:-sekka-cache}
    profile=''${SEKKA_CACHE_PROFILE:-sekka-cache}
    if [[ ! -r $key ]]; then
      echo "sekka-push-cache: signing key $key is missing" >&2
      exit 1
    fi
    # The nightly workflow gives its token in the environment; by hand, it
    # is the AWS profile.
    if [[ -n ''${AWS_ACCESS_KEY_ID:-} ]]; then
      s3() { aws --region auto --endpoint-url "$endpoint" s3 "$@"; }
    else
      s3() { aws --profile "$profile" --region auto --endpoint-url "$endpoint" s3 "$@"; }
    fi

    # Each of these is evaluated with Nixpkgs' default configuration, which
    # refuses unfree packages, so nothing unfree is published.
    # installed-systems is an installed system for every desktop, refusing
    # unfree packages too: everything they need that cache.nixos.org lacks.
    # Uta is an explicit publication root even though it is not preloaded on
    # the ISO. Its build is GPL-3.0-only, so uta.source, the exact sources it
    # was built from (nix/uta.nix), goes beside it. Keep a failed build from
    # being hidden by process substitution.
    packages=(genkan tatami displays wallpapers yukimi kakehashi uta uta.source aquamarine hyprland cargo-geiger tools installed-systems)
    if [[ -n ''${SEKKA_CI_ROOTS:-} ]]; then
      build_outputs=$(python3 ${../scripts/ci-retain.py} main packages build --no-update-lock-file --print-out-paths "''${packages[@]/#/$flake#}")
    else
      build_outputs=$(nix build --no-update-lock-file --no-link --print-out-paths "''${packages[@]/#/$flake#}")
    fi
    mapfile -t outputs <<< "$build_outputs"

    # Query caches before exporting anything. Stage only missing paths, with
    # Nix's signatures; references already served upstream need no archive
    # here. The workflow's separate coverage gate still checks the closure.
    # SEKKA_CACHE_URL is where the bucket is read from; set with
    # SEKKA_CACHE_BUCKET for a test cache, as the two must be the same one or
    # paths the bucket lacks look published.
    stage=$(mktemp -d)
    trap 'rm -rf "$stage"' EXIT
    destination=()
    if [[ -n ''${SEKKA_CACHE_URL:-} ]]; then
      destination=(--destination "$SEKKA_CACHE_URL")
    fi
    python3 ${preparation}/prepare_cache.py --stage "$stage/upload" --key-file "$key" \
      "''${destination[@]}" "''${outputs[@]}"

    # All archives before any narinfos. Only missing/broken paths were staged;
    # copying those directly avoids listing the whole bucket and also repairs
    # existing narinfos even when the new metadata has the same byte length.
    s3 cp --recursive --no-progress --content-type application/x-nix-nar \
      "$stage/upload/nar" "s3://$bucket/nar"
    s3 cp --recursive --no-progress --content-type text/x-nix-narinfo \
      --exclude '*' --include '*.narinfo' "$stage/upload" "s3://$bucket"
    # Nix writes only StoreDir when it creates a cache; without a priority the
    # cache would be asked before cache.nixos.org (40).
    printf 'StoreDir: /nix/store\nWantMassQuery: 1\nPriority: 50\n' |
      s3 cp - "s3://$bucket/nix-cache-info" --content-type text/x-nix-cache-info
    echo "Published the staged paths; the workflow verifies complete runtime coverage next"
  '';
  meta.description = "Upload Sekka's own packages to https://cache.sekka.computer";
}
