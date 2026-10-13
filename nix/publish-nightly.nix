# `nix run .#publish-nightly ISO_DIRECTORY`: uploads the ISO built from this
# checkout to https://downloads.sekka.computer/nightly/ (Cloudflare R2 bucket
# sekka-downloads), replacing the last nightly. The nightly workflow runs it
# with a token that can write that bucket alone, in AWS_ACCESS_KEY_ID and
# AWS_SECRET_ACCESS_KEY (DEVELOPERS.md).
{ pkgs }:
pkgs.writeShellApplication {
  name = "sekka-publish-nightly";
  runtimeInputs = with pkgs; [
    awscli2
    coreutils
    git
    jq
  ];
  text = ''
    source=''${1:?Usage: sekka-publish-nightly ISO_DIRECTORY}
    endpoint=''${SEKKA_DOWNLOADS_ENDPOINT:-https://c408b91e58b3aa56c4f71046fcd416b6.r2.cloudflarestorage.com}
    bucket=sekka-downloads
    name=sekka-nightly-x86_64-linux.iso

    shopt -s nullglob
    images=("$source"/*.iso)
    if [[ ''${#images[@]} -ne 1 ]]; then
      echo "sekka-publish-nightly: expected exactly one ISO in $source" >&2
      exit 1
    fi
    iso=''${images[0]}
    if [[ -n $(git status --porcelain --untracked-files=no) ]]; then
      echo "sekka-publish-nightly: the checkout has changes; publish only what main holds" >&2
      exit 1
    fi

    stage=$(mktemp -d)
    trap 'rm -rf "$stage"' EXIT
    sha=$(sha256sum "$iso" | cut -d' ' -f1)
    printf '%s  %s\n' "$sha" "$name" >"$stage/SHA256SUMS"
    cp flake.lock "$stage/flake.lock"
    jq -n \
      --arg revision "$(git rev-parse HEAD)" \
      --arg committed "$(git show -s --format=%cI HEAD)" \
      --arg built "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
      --arg iso "$name" \
      --argjson bytes "$(stat -c %s "$iso")" \
      --arg sha256 "$sha" \
      '{revision: $revision, committed: $committed, built: $built,
        system: "x86_64-linux", iso: $iso, bytes: $bytes, sha256: $sha256}' \
      >"$stage/manifest.json"

    s3() { aws --region auto --endpoint-url "$endpoint" s3 "$@"; }
    # The image first, then what describes it: until the manifest names the
    # new revision, the next run would build and upload it again.
    s3 cp --no-progress --content-type application/x-iso9660-image \
      --cache-control 'public, max-age=300' "$iso" "s3://$bucket/nightly/$name"
    s3 cp --no-progress --content-type 'text/plain; charset=utf-8' \
      --cache-control no-cache "$stage/SHA256SUMS" "s3://$bucket/nightly/SHA256SUMS"
    for file in flake.lock manifest.json; do
      s3 cp --no-progress --content-type application/json \
        --cache-control no-cache "$stage/$file" "s3://$bucket/nightly/$file"
    done
    echo "Published $(jq -r .revision "$stage/manifest.json") as https://downloads.sekka.computer/nightly/$name"
  '';
  meta.description = "Upload a nightly Sekka ISO to https://downloads.sekka.computer/nightly/";
}
