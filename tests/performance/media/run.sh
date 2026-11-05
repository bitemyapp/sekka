#!/usr/bin/env bash
# Run a media benchmark plan (bench.py) or the time to first frame (ttff.py)
# under the measurement lock.
#
#   tests/performance/media/run.sh PLAN.json OUT_DIR [bench.py args…]
#   tests/performance/media/run.sh ttff CLIP OUT_DIR [ttff.py args…]
#
# Resolves every program from this checkout's flake (Aizome) and its nixpkgs
# (players, sway, PipeWire tools, D-Bus, Python), then runs the script under
# tests/performance/lock-measure (LOCK=build uses lock-build, for
# smoke runs; LOCK=none runs it bare, for a caller that holds the lock).
# Clips come from $CLIPS (encode.sh).
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../../.." && pwd)
perf=${PERF_DIR:-${TMPDIR:-/tmp}/aizome-perf}

build() {
  nix build --no-link --print-out-paths --inputs-from "$repo" "$1^out" 2>/dev/null | head -1
}
export AIZOME=${AIZOME:-$(nix build --no-link --print-out-paths "$repo#aizome" 2>/dev/null)}
export MPV=${MPV:-$(build nixpkgs#mpv)}
export VLC=${VLC:-$(build nixpkgs#vlc)}
export SWAY=${SWAY:-$(build nixpkgs#sway)}
export XWAYLAND_SATELLITE=${XWAYLAND_SATELLITE:-$(build nixpkgs#xwayland-satellite)}
export DBUS=${DBUS:-$(build nixpkgs#dbus)}
export PIPEWIRE=${PIPEWIRE:-$(build nixpkgs#pipewire)}
export GRIM=${GRIM:-$(build nixpkgs#grim)}
export SHOWTIME=${SHOWTIME:-$(build nixpkgs#showtime)}
export HARUNA=${HARUNA:-$(build nixpkgs#haruna)}
python=${PYTHON:-$(build nixpkgs#python3)/bin/python3}
export TATAMI_MPV_CONF=$repo/desktop/tatami/mpv.conf
export CLIPS=${CLIPS:-$perf/media/clips}
export PATH=$SWAY/bin:$PATH  # sway's Xwayland and swaymsg

if [ "$1" = ttff ]; then
  clip=$2
  out=$3
  shift 3
  cmd=("$python" -I "$here/ttff.py" --clip "$clip" --out "$out" "$@")
else
  plan=$1
  out=$2
  shift 2
  cmd=("$python" -I "$here/bench.py" --plan "$plan" --out "$out" "$@")
fi

mkdir -p "$out"
{
  echo "aizome=$AIZOME"
  echo "mpv=$MPV"
  echo "vlc=$VLC"
  echo "sway=$SWAY"
  echo "xwayland_satellite=$XWAYLAND_SATELLITE"
  echo "dbus=$DBUS"
  echo "pipewire=$PIPEWIRE"
  echo "showtime=$SHOWTIME"
  echo "haruna=$HARUNA"
  echo "python=$python"
  echo "commit=$(git -C "$repo" rev-parse HEAD)"
} > "$out/programs.txt"

case "${LOCK:-measure}" in
  measure) exec "$here/../lock-measure" "${cmd[@]}" ;;
  build) exec "$here/../lock-build" "${cmd[@]}" ;;
  none) exec "${cmd[@]}" ;;
esac
