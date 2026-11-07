#!/bin/bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Build Sekka Installer.app (with its helper) and Sekka Recovery.app on a Mac.
#
#   packaging/build-app.sh --engine ENGINE.tar.gz [--icon AppIcon.icns] [--out DIR]
#   packaging/build-app.sh --release --engine ENGINE.tar.gz --icon AppIcon.icns --out DIR
#
# Development builds (the default) carry the simulation, trust only the
# development catalog key, and are signed ad hoc: they run on the Mac that
# built them and prove nothing about a release. --release builds the
# production features and signs with the Developer ID named by
# SEKKA_SIGNING_IDENTITY for team SEKKA_TEAM_ID, with the hardened runtime;
# notarize the result with packaging/notarize.sh. Every Mach-O must be
# signed by SEKKA_TEAM_ID except the Python runtime inside the engine
# archive, which is Asahi's, signed by its own team and never re-signed
# (that would change the engine's digest): a release names that team as
# SEKKA_ENGINE_TEAM_ID, which must be the runtime_team that
# engine/source-lock.json pins. The one runtime file Asahi ships signed ad
# hoc (libffi) is accepted by the exact digest the lock records as
# runtime_adhoc, and nothing else is. The engine archive and icon come from the
# root flake:
#
#   nix build .#packages.aarch64-linux.mac-engine .#packages.aarch64-linux.mac-installer-icon
#
# Output: DIR/Sekka Installer.app, DIR/recovery.zip (the Recovery app, a
# release artifact of its own) and DIR/build.json (what was built, from what).
# DIR (target/package by default) is replaced, so it must not exist yet or
# be a directory an earlier run made, which its .sekka-build-app file
# marks; anything else is refused before anything is built.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
workspace=$(cd "$here/.." && pwd)
release=false
engine=""
icon=""
out="$workspace/target/package"
while (( $# )); do
  case "$1" in
    --release) release=true; shift ;;
    --engine) engine=$2; shift 2 ;;
    --icon) icon=$2; shift 2 ;;
    --out) out=$2; shift 2 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done

# What a build is given is checked before anything is built.
if $release; then
  : "${SEKKA_TEAM_ID:?a release names the team that signs it (SEKKA_TEAM_ID)}"
  : "${SEKKA_SIGNING_IDENTITY:?a release names its Developer ID Application identity (SEKKA_SIGNING_IDENTITY)}"
  : "${SEKKA_ENGINE_TEAM_ID:?a release names the team that signed the Python runtime in the engine (SEKKA_ENGINE_TEAM_ID, as engine/source-lock.json pins it)}"
  pinned=$(python3 -I -c 'import json, sys; print(json.load(open(sys.argv[1]))["asahi_installer"]["runtime_team"])' "$workspace/engine/source-lock.json")
  if [[ "$SEKKA_ENGINE_TEAM_ID" != "$pinned" ]]; then
    echo "SEKKA_ENGINE_TEAM_ID is $SEKKA_ENGINE_TEAM_ID, but engine/source-lock.json pins the runtime's team as $pinned." >&2
    exit 1
  fi
fi

# The output directory is deleted and made again, so only one that doesn't
# exist or that this script made (and marked) may be named.
marker=.sekka-build-app
check_out() {
  if [[ -z "$out" ]]; then
    echo "--out names a directory." >&2
    exit 2
  elif [[ -L "$out" ]]; then
    echo "--out $out is a symbolic link; name a new directory, which build-app.sh makes." >&2
    exit 2
  elif [[ -e "$out" && ! ( -d "$out" && -f "$out/$marker" ) ]]; then
    echo "--out $out exists and wasn't made by build-app.sh (it has no $marker), and build-app.sh replaces it; name a new directory." >&2
    exit 2
  fi
}
check_out
if [[ -d "$out" ]]; then
  out_real=$(cd "$out" && pwd -P)
  for input in "$engine" "$icon"; do
    [[ -n "$input" && -e "$input" ]] || continue
    input_dir=$(cd "$(dirname "$input")" && pwd -P)
    case "$input_dir/" in
      "$out_real"/*)
        echo "$input is inside --out $out, which build-app.sh replaces; keep --engine and --icon elsewhere." >&2
        exit 2 ;;
    esac
  done
fi

if [[ "$(uname -s)" != Darwin ]]; then
  echo "Sekka Installer is built on macOS. (Its tests run anywhere: cargo test.)" >&2
  exit 2
fi
[[ -f "$engine" ]] || { echo "--engine names the engine archive (nix build .#packages.aarch64-linux.mac-engine)" >&2; exit 2; }

version=$(sed -n 's/^version = "\(.*\)"/\1/p' "$workspace/Cargo.toml" | head -1)
revision=$(git -C "$workspace" rev-parse HEAD 2>/dev/null || echo unknown)
if ! git -C "$workspace" diff --quiet HEAD -- . 2>/dev/null; then
  if $release; then
    echo "A release is built from a clean checkout; commit or discard changes first." >&2
    exit 1
  fi
  revision="$revision-dirty"
fi
build=$(date -u +%Y%m%d%H%M)

features=()
if $release; then
  features=(--no-default-features --features production)
  profile=release
else
  unset SEKKA_TEAM_ID || true
  profile=release
fi

target=aarch64-apple-darwin
export MACOSX_DEPLOYMENT_TARGET=14.0
cargo build --locked --release --target "$target" -p sekka-installer ${features[@]+"${features[@]}"} --manifest-path "$workspace/Cargo.toml"
helper_features=()
$release && helper_features=(--features production)
cargo build --locked --release --target "$target" -p sekka-mac-helper ${helper_features[@]+"${helper_features[@]}"} --manifest-path "$workspace/Cargo.toml"
# Recovery stubs can be older than the Mac's macOS.
MACOSX_DEPLOYMENT_TARGET=13.0 cargo build --locked --release --target "$target" -p sekka-recovery --manifest-path "$workspace/Cargo.toml"
bin="$workspace/target/$target/$profile"

check_out
rm -rf "$out"
mkdir -p "$out"
echo "Made by packaging/build-app.sh, which replaces this directory when --out names it again." > "$out/$marker"
fill() { sed -e "s/@VERSION@/$version/" -e "s/@BUILD@/$build/" -e "s/@REVISION@/$revision/" "$1" > "$2"; }

app="$out/Sekka Installer.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources/Engine" "$app/Contents/Library/LaunchDaemons"
fill "$here/app/Info.plist.in" "$app/Contents/Info.plist"
cp "$bin/sekka-installer" "$app/Contents/MacOS/sekka-installer"
cp "$bin/sekka-installer-helper" "$app/Contents/MacOS/sekka-installer-helper"
cp "$here/app/computer.sekka.installer.helper.plist" "$app/Contents/Library/LaunchDaemons/"
cp "$engine" "$app/Contents/Resources/Engine/engine.tar.gz"
if [[ -n "$icon" ]]; then
  cp "$icon" "$app/Contents/Resources/AppIcon.icns"
elif $release; then
  echo "A release carries its icon (--icon)." >&2
  exit 1
fi

recovery="$out/Sekka Recovery.app"
mkdir -p "$recovery/Contents/MacOS" "$recovery/Contents/Resources/en.lproj"
fill "$here/recovery/Info.plist.in" "$recovery/Contents/Info.plist"
cp "$here/recovery/en.lproj/InfoPlist.strings" "$recovery/Contents/Resources/en.lproj/"
cp "$bin/sekka-recovery" "$recovery/Contents/MacOS/sekka-recovery"

# Inside out: the helper and the Recovery app before the app that holds the
# helper. Each binary is signed with its own identifier, which the other
# side's code-signing requirement names.
if $release; then
  sign=(codesign --force --options runtime --timestamp --sign "$SEKKA_SIGNING_IDENTITY")
else
  sign=(codesign --force --sign -)
fi
"${sign[@]}" --identifier computer.sekka.installer.helper "$app/Contents/MacOS/sekka-installer-helper"
"${sign[@]}" --identifier computer.sekka.installer "$app"
"${sign[@]}" --identifier computer.sekka.installer.recovery "$recovery"
codesign --verify --deep --strict "$app"
codesign --verify --deep --strict "$recovery"

# The Recovery app ships beside the app, as a release artifact the engine
# places on the new system's stub.
(cd "$out" && ditto -c -k --keepParent "Sekka Recovery.app" recovery.zip)

audit=(--verify)
if $release; then
  audit+=(--team "$SEKKA_TEAM_ID" --archive-team "engine.tar.gz=$SEKKA_ENGINE_TEAM_ID")
  while read -r digest; do
    audit+=(--allow-adhoc "engine.tar.gz=$digest")
  done < <(python3 -I -c 'import json, sys
for f in json.load(open(sys.argv[1]))["asahi_installer"].get("runtime_adhoc", []):
    print(f["sha256"])' "$workspace/engine/source-lock.json")
fi
python3 "$here/audit-macho.py" "${audit[@]}" "$app" "$out/recovery.zip" > "$out/macho-audit.json"
python3 "$here/provenance.py" --workspace "$workspace" --release "$release" \
  --artifact "$app" --artifact "$out/recovery.zip" --artifact "$engine" > "$out/build.json"
echo "$out"
