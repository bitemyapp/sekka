# `nix run .#push-cache [FLAKE]`: uploads Sekka's own packages to the public
# binary cache at https://cache.sekka.computer (Cloudflare R2 bucket
# sekka-cache), leaving the rest of their closures to cache.nixos.org.
# DEVELOPERS.md describes the key and credentials it expects. The cache
# workflow runs it for every push to main, the nightly workflow for every
# revision it publishes.
{ pkgs }:
pkgs.writeShellApplication {
  name = "sekka-push-cache";
  # Building and signing use the caller's own Nix and daemon.
  runtimeInputs = [ pkgs.awscli2 ];
  text = ''
    flake=''${1:-.}
    config=''${SEKKA_CACHE_CONFIG:-''${XDG_CONFIG_HOME:-$HOME/.config}/sekka-cache}
    key=$config/cache.sekka.computer-1.sec
    endpoint=''${SEKKA_CACHE_ENDPOINT:-https://c408b91e58b3aa56c4f71046fcd416b6.r2.cloudflarestorage.com}
    bucket=sekka-cache
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
    # the ISO. Keep a failed build from being hidden by process substitution.
    packages=(genkan tatami displays wallpapers yukimi kakehashi uta aquamarine hyprland tools installed-systems)
    build_outputs=$(nix build --no-update-lock-file --no-link --print-out-paths "''${packages[@]/#/$flake#}")
    mapfile -t outputs <<< "$build_outputs"

    # Nix only adds a path to a binary cache that already holds its
    # references, so sign and compress the complete closure locally first.
    stage=$(mktemp -d)
    trap 'rm -rf "$stage"' EXIT
    nix copy --to "file://$stage/all?compression=zstd&secret-key=$key" "''${outputs[@]}"

    # Publish only what no cache Sekka systems use already serves:
    # cache.nixos.org, Determinate's (its Nix) and numtide's (the AI
    # applications). Clients substitute each reference separately, from
    # whichever cache has it. Determinate Nixd comes from Determinate alone
    # (FlakeHub's cache, or the installation media); it is theirs to publish.
    mapfile -t closure < <(nix path-info --recursive "''${outputs[@]}")
    own=("''${closure[@]}")
    for upstream in https://cache.nixos.org https://install.determinate.systems https://cache.numtide.com; do
      mapfile -t own < <(nix-store --store "$upstream" --check-validity --print-invalid "''${own[@]}")
    done
    mapfile -t own < <(printf '%s\n' "''${own[@]}" | grep -v -e '-determinate-nixd-' -e '^$' || true)
    mkdir -p "$stage/upload/nar"
    for path in "''${own[@]}"; do
      name=''${path#/nix/store/}
      narinfo=''${name%%-*}.narinfo
      nar=$(sed -n 's/^URL: //p' "$stage/all/$narinfo")
      ln -f "$stage/all/$narinfo" "$stage/upload/$narinfo"
      ln -f "$stage/all/$nar" "$stage/upload/$nar"
    done

    # Archives before the narinfos that point to them. Store paths never
    # change, so objects already in the bucket are skipped.
    s3 sync --size-only --no-progress --content-type application/x-nix-nar \
      "$stage/upload/nar" "s3://$bucket/nar"
    s3 sync --size-only --no-progress --content-type text/x-nix-narinfo \
      --exclude '*' --include '*.narinfo' "$stage/upload" "s3://$bucket"
    # Nix writes only StoreDir when it creates a cache; without a priority the
    # cache would be asked before cache.nixos.org (40).
    printf 'StoreDir: /nix/store\nWantMassQuery: 1\nPriority: 50\n' |
      s3 cp - "s3://$bucket/nix-cache-info" --content-type text/x-nix-cache-info
    echo "Published ''${#own[@]} of ''${#closure[@]} paths; the other caches serve the rest"
  '';
  meta.description = "Upload Sekka's own packages to https://cache.sekka.computer";
}
