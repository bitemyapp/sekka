# `nix run .#publish-nightly ISO_DIRECTORY`: uploads the ISO built from this
# checkout to https://downloads.sekka.computer/nightly/ (Cloudflare R2 bucket
# sekka-downloads) as a new build, and keeps the last seven. The nightly
# workflow runs it with a token that can write that bucket alone, in
# AWS_ACCESS_KEY_ID and AWS_SECRET_ACCESS_KEY (DEVELOPERS.md).
#
# Each build has a folder of its own, nightly/builds/DATE-REVISION/, with
# the ISO, SHA256SUMS, flake.lock and manifest.json, none of which ever
# change. nightly/builds.json lists the kept builds, newest first; it is
# written last, and then everything else under nightly/ is removed.
{ pkgs }:
pkgs.writeShellApplication {
  name = "sekka-publish-nightly";
  runtimeInputs = with pkgs; [
    awscli2
    coreutils
    git
    gnugrep
    jq
  ];
  text = ''
    source=''${1:?Usage: sekka-publish-nightly ISO_DIRECTORY}
    endpoint=''${SEKKA_DOWNLOADS_ENDPOINT:-https://c408b91e58b3aa56c4f71046fcd416b6.r2.cloudflarestorage.com}
    bucket=sekka-downloads
    keep=7

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

    s3() { aws --region auto --endpoint-url "$endpoint" s3 "$@"; }
    s3api() { aws --region auto --endpoint-url "$endpoint" s3api "$@"; }
    stage=$(mktemp -d)
    trap 'rm -rf "$stage"' EXIT

    # The list as it stands: none yet is an empty list, but any other
    # failure stops here rather than forget the builds already published.
    if ! s3api get-object --bucket "$bucket" --key nightly/builds.json "$stage/current.json" \
      >/dev/null 2>"$stage/error"; then
      if grep -q NoSuchKey "$stage/error"; then
        echo '{"builds": []}' >"$stage/current.json"
      else
        cat "$stage/error" >&2
        exit 1
      fi
    fi

    revision=$(git rev-parse HEAD)
    id="$(date -u +%Y%m%d)-$(git rev-parse --short=7 HEAD)"
    folder="builds/$id"
    name="sekka-nightly-$id-x86_64-linux.iso"
    sha=$(sha256sum "$iso" | cut -d' ' -f1)
    mkdir "$stage/build"
    printf '%s  %s\n' "$sha" "$name" >"$stage/build/SHA256SUMS"
    cp flake.lock "$stage/build/flake.lock"

    forever='public, max-age=31536000, immutable'
    s3 cp --no-progress --content-type application/x-iso9660-image \
      --cache-control "$forever" "$iso" "s3://$bucket/nightly/$folder/$name"
    uploaded=$(date -u +%Y-%m-%dT%H:%M:%SZ)
    jq -n \
      --arg id "$id" \
      --arg revision "$revision" \
      --arg committed "$(git show -s --format=%cI HEAD)" \
      --arg uploaded "$uploaded" \
      --arg iso "$folder/$name" \
      --arg sums "$folder/SHA256SUMS" \
      --arg manifest "$folder/manifest.json" \
      --arg lock "$folder/flake.lock" \
      --argjson bytes "$(stat -c %s "$iso")" \
      --arg sha256 "$sha" \
      '{id: $id, revision: $revision, committed: $committed, uploaded: $uploaded,
        system: "x86_64-linux", iso: $iso, sha256sums: $sums, manifest: $manifest,
        flake_lock: $lock, bytes: $bytes, sha256: $sha256}' \
      >"$stage/build/manifest.json"
    s3 cp --no-progress --content-type 'text/plain; charset=utf-8' \
      --cache-control "$forever" "$stage/build/SHA256SUMS" "s3://$bucket/nightly/$folder/SHA256SUMS"
    for file in flake.lock manifest.json; do
      s3 cp --no-progress --content-type application/json \
        --cache-control "$forever" "$stage/build/$file" "s3://$bucket/nightly/$folder/$file"
    done

    # The new build first, then the newest of the rest, up to $keep.
    jq --slurpfile new "$stage/build/manifest.json" --argjson keep "$keep" \
      '{builds: ($new + [.builds[] | select(.id != $new[0].id)])[:$keep]}' \
      "$stage/current.json" >"$stage/builds.json"
    s3 cp --no-progress --content-type application/json --cache-control no-cache \
      "$stage/builds.json" "s3://$bucket/nightly/builds.json"

    # nightly/ holds the list and the kept builds' folders, nothing else.
    jq -r '.builds[].id' "$stage/builds.json" >"$stage/kept"
    s3 ls --recursive "s3://$bucket/nightly/" | while read -r _ _ _ key; do
      case $key in
        nightly/builds.json) continue ;;
        nightly/builds/*/*)
          build=''${key#nightly/builds/}
          grep -qxF "''${build%%/*}" "$stage/kept" && continue
          ;;
      esac
      s3 rm --only-show-errors "s3://$bucket/$key"
      echo "Removed $key"
    done
    echo "Published $id: https://downloads.sekka.computer/nightly/$folder/$name"
  '';
  meta.description = "Upload a nightly Sekka ISO to https://downloads.sekka.computer/nightly/ and keep the last seven";
}
