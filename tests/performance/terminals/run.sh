#!/usr/bin/env bash
# Run one phase of the terminal comparison (docs/performance/terminals.md).
#
#   tests/performance/terminals/run.sh env              # once: shell environment, observer, corpora
#   tests/performance/terminals/run.sh calibrate
#   tests/performance/terminals/run.sh startup 10       # PHASE [ROUNDS [START_ROUND [SUFFIX]]]
#   tests/performance/terminals/run.sh steady 2
#   tests/performance/terminals/run.sh throughput 4
#   tests/performance/terminals/run.sh energy 3
#   tests/performance/terminals/run.sh idle-default 3
#
# With LOCKS set to a directory holding `lock-measure` and `lock-build`, every
# phase but `env` runs inside the measurement lock (calibrate inside the build
# lock: its timings aren't reported), keeping other work off the machine;
# without it, phases just run.
# WORK holds the environment, observer, corpora, warm caches and raw output.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../../.." && pwd)
WORK=${WORK:-${TMPDIR:-/tmp}/aizome-perf/terminals}
APPS=${APPS:-hotaru,ghostty,gnome-terminal,konsole,kgx,foot}
LOCKS=${LOCKS:-}
locked() { local kind=$1; shift; if [ -n "$LOCKS" ]; then exec "$LOCKS/lock-$kind" "$@"; else exec "$@"; fi; }
phase=$1
rounds=${2:-1}
start=${3:-0}
suffix=${4:-}

if [ "$phase" = env ]; then
  mkdir -p "$WORK"
  nixpkgs=$(nix eval --raw --inputs-from "$repo" nixpkgs#path)
  hotaru=$(nix build --no-link --print-out-paths "$repo#hotaru")
  fonts=$(nix build --no-link --print-out-paths "$repo#aizome-fonts")
  nix-shell "$here/shell.nix" --arg nixpkgs "$nixpkgs" --argstr hotaru "$hotaru" --argstr fonts "$fonts" \
    --run 'export -p' > "$WORK/shell-env.sh"
  # shellcheck disable=SC1091
  source "$WORK/shell-env.sh"
  python3 - "$here" "$WORK/observer" <<'EOF'
import importlib.util, os, sys
from pathlib import Path
spec = importlib.util.spec_from_file_location("bench", Path(sys.argv[1]) / "terminal-bench.py")
bench = importlib.util.module_from_spec(spec); spec.loader.exec_module(bench)
bench.build_observer(Path(sys.argv[2]), {"screencopy": Path(os.environ["SCREENCOPY_XML"]),
                                         "virtual-keyboard": Path(os.environ["VIRTUAL_KEYBOARD_XML"])})
EOF
  python3 "$here/corpus.py" "$WORK/corpora" --bytes 8388608
  exit 0
fi

# shellcheck disable=SC1091
source "$WORK/shell-env.sh"
# Hotaru's fallback fonts: the folder its own wrapper names.
fallback=$(sed -n "s/^export HOTARU_FONT_DIR=\${HOTARU_FONT_DIR-'\(.*\)'}$/\1/p" "$TERMBENCH_HOTARU")
mkdir -p "$WORK/fc-cache"
common=(--observer "$WORK/observer" --font-dir "$TERMBENCH_FONTS" --fallback-dir "$fallback"
        --fontconfig-cache "$WORK/fc-cache" --apps "$APPS")
binaries=(--binary "hotaru=$TERMBENCH_HOTARU" --binary "ghostty=$TERMBENCH_GHOSTTY"
          --binary "gnome-terminal=$TERMBENCH_GNOME_TERMINAL/bin/gnome-terminal" --binary "konsole=$TERMBENCH_KONSOLE"
          --binary "kgx=$TERMBENCH_KGX" --binary "foot=$TERMBENCH_FOOT" --binary "alacritty=$TERMBENCH_ALACRITTY"
          --binary "kitty=$TERMBENCH_KITTY")
# A second Ghostty build (its main branch), and the history the matched
# configuration gives Hotaru and Ghostty (SCROLLBACK=ghostty-default).
[ -n "${GHOSTTY_MAIN:-}" ] && binaries+=(--binary "ghostty-main=$GHOSTTY_MAIN")
common+=(--scrollback "${SCROLLBACK:-zero}")

# CALIBRATION names the calibration directory (one per set of terminals).
cal=${CALIBRATION:-$WORK/calibration}
if [ "$phase" = calibrate ]; then
  rm -rf "$cal"
  locked build python3 "$here/terminal-matrix.py" --phase calibrate --output "$cal" \
    "${common[@]}" "${binaries[@]}"
fi

extra=()
if [ "$phase" = throughput ]; then
  for c in ascii styled unicode cursor; do extra+=(--corpus "$WORK/corpora/$c.vt"); done
fi
# DATA names the data root (one per configuration: the summary pools every
# run of a phase under it).
out="${DATA:-$WORK/data}/$phase$suffix"
locked measure bash -c '
  echo "load before: $(cat /proc/loadavg)"
  exec "$@"' _ python3 "$here/terminal-matrix.py" --phase "$phase" --output "$out" \
  --calibration "$cal/calibration.json" "${common[@]}" --rounds "$rounds" --start-round "$start" "${extra[@]}"
