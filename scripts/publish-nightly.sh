#!/usr/bin/env bash
# Wrapped by nix/publish-nightly.nix, with its tools on PATH.
# Stage both architectures before publishing the index; old x86-only entries
# and their download paths remain valid.
set -euo pipefail
if [[ $# -ne 2 ]]; then
  echo "Usage: sekka-publish-nightly X86_ISO_DIRECTORY ARM_ISO_DIRECTORY" >&2
  exit 1
fi
endpoint=${SEKKA_DOWNLOADS_ENDPOINT:-https://c408b91e58b3aa56c4f71046fcd416b6.r2.cloudflarestorage.com}
bucket=sekka-downloads
keep=7
systems=(x86_64-linux aarch64-linux)
sources=("$1" "$2")
images=()
shopt -s nullglob
for i in "${!systems[@]}"; do
  candidates=("${sources[i]}"/*.iso)
  if [[ ${#candidates[@]} -ne 1 || ${candidates[0]} != *-${systems[i]}.iso || ! -s ${candidates[0]} ]]; then
    echo "sekka-publish-nightly: expected one nonempty ${systems[i]} ISO in ${sources[i]}" >&2
    exit 1
  fi
  images+=("${candidates[0]}")
done
if [[ -n $(git status --porcelain --untracked-files=no) ]]; then
  echo "sekka-publish-nightly: the checkout has changes; publish only what main holds" >&2
  exit 1
fi

s3() { aws --region auto --endpoint-url "$endpoint" s3 "$@"; }
s3api() { aws --region auto --endpoint-url "$endpoint" s3api "$@"; }
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT

# Missing index means no builds yet; any other failure must not erase history.
if ! s3api get-object --bucket "$bucket" --key nightly/builds.json "$stage/current.json" \
  >/dev/null 2>"$stage/error"; then
  if grep -q NoSuchKey "$stage/error"; then
    echo '{"builds": []}' >"$stage/current.json"
  else
    cat "$stage/error" >&2
    exit 1
  fi
fi
jq -e '.builds | type == "array"' "$stage/current.json" >/dev/null

revision=$(git rev-parse HEAD)
id="$(date -u +%Y%m%d)-$(git rev-parse --short=7 HEAD)"
folder="builds/$id"
mkdir -p "$stage/build/aarch64-linux"
cp flake.lock "$stage/build/flake.lock"
: >"$stage/images.jsonl"
for i in "${!systems[@]}"; do
  system=${systems[i]}
  name="sekka-nightly-$id-$system.iso"
  # Preserve x86 URLs, including its single-image SHA256SUMS. ARM has its own
  # checksum file, so verifying either download does not require the other ISO.
  relative=$name
  sums=SHA256SUMS
  if [[ $system == aarch64-linux ]]; then
    relative="$system/$name"
    sums="$system/SHA256SUMS"
  fi
  sha=$(sha256sum "${images[i]}" | cut -d' ' -f1)
  printf '%s  %s\n' "$sha" "$name" >"$stage/build/$sums"
  jq -n --arg system "$system" --arg iso "$folder/$relative" \
    --arg sums "$folder/$sums" --arg sha256 "$sha" \
    --argjson bytes "$(stat -c %s "${images[i]}")" \
    '{system: $system, iso: $iso, sha256sums: $sums, bytes: $bytes, sha256: $sha256}' \
    >>"$stage/images.jsonl"
done

forever='public, max-age=31536000, immutable'
for i in "${!systems[@]}"; do
  relative=$(jq -sr --argjson i "$i" '.[$i].iso' "$stage/images.jsonl")
  sums=$(jq -sr --argjson i "$i" '.[$i].sha256sums' "$stage/images.jsonl")
  s3 cp --no-progress --content-type application/x-iso9660-image \
    --cache-control "$forever" "${images[i]}" "s3://$bucket/nightly/$relative"
  s3 cp --no-progress --content-type 'text/plain; charset=utf-8' \
    --cache-control "$forever" "$stage/build/${sums#"$folder/"}" "s3://$bucket/nightly/$sums"
done
jq -n --arg id "$id" --arg revision "$revision" \
  --arg committed "$(git show -s --format=%cI HEAD)" \
  --arg uploaded "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
  --arg manifest "$folder/manifest.json" --arg lock "$folder/flake.lock" \
  --slurpfile images "$stage/images.jsonl" \
  '$images[0] + {id: $id, revision: $revision, committed: $committed,
    uploaded: $uploaded, manifest: $manifest, flake_lock: $lock, images: $images}' \
  >"$stage/build/manifest.json"
for file in flake.lock manifest.json; do
  s3 cp --no-progress --content-type application/json --cache-control "$forever" \
    "$stage/build/$file" "s3://$bucket/nightly/$folder/$file"
done

# One revision is one build, with both architectures. The old index remains
# visible if either ISO, checksum or shared metadata upload fails.
jq --slurpfile new "$stage/build/manifest.json" --argjson keep "$keep" \
  '{builds: ($new + [.builds[] | select(.id != $new[0].id)])[:$keep]}' \
  "$stage/current.json" >"$stage/builds.json"
s3 cp --no-progress --content-type application/json --cache-control no-cache \
  "$stage/builds.json" "s3://$bucket/nightly/builds.json"

jq -r '.builds[].id' "$stage/builds.json" >"$stage/kept"
s3 ls --recursive "s3://$bucket/nightly/" | while read -r _ _ _ key; do
  case $key in
    nightly/builds.json) continue ;;
    nightly/builds/*/*)
      build=${key#nightly/builds/}
      grep -qxF "${build%%/*}" "$stage/kept" && continue
      ;;
  esac
  s3 rm --only-show-errors "s3://$bucket/$key"
  echo "Removed $key"
done
echo "Published both architectures: https://downloads.sekka.computer/nightly/$folder/manifest.json"
