#!/usr/bin/env bash
# budget.sh OUTDIR TARGET_DIR FONT_DIR
#
# The 4 ms budget on this machine: Aizome's own headless frame-budget tests
# (desktop/aizome/docs/performance.md), release builds, each timing frames
# from an event through update, view, layout, diff and rasterising the
# damage at 2x through the kit's offscreen renderer. Builds first (under
# LOCK_BUILD, if set), then runs each test once under LOCK, one at a time,
# writing its output to OUTDIR/<name>.txt.
#
#   LOCK_BUILD=lock-build LOCK=lock-measure \
#     tests/performance/design/budget.sh docs/performance/data/design/budget \
#     /tmp/target "$AIZOME_FONT_DIR"
set -euo pipefail
OUT=$(realpath -m "$1"); TARGET=$2; FONTS=$3
REPO=$(cd "$(dirname "$0")/../../.." && pwd)
mkdir -p "$OUT"
cd "$REPO/desktop/aizome"

# name | package | cargo test arguments after the package
TESTS=(
  "kit|aizome-kit|--test perf -- --ignored --nocapture --test-threads 1"
  "shell|aizome-shell|--test perf -- --nocapture --test-threads 1"
  "sumi|aizome-sumi|--test sumi typing_and_scrolling_fit_the_frame_budget -- --nocapture --test-threads 1"
  "hako|aizome-hako|--test perf -- --nocapture --test-threads 1"
  "myaku|aizome-myaku|--test perf -- --nocapture --test-threads 1"
)

dev() { nix develop "$REPO#aizome" -c env CARGO_TARGET_DIR="$TARGET" AIZOME_FONT_DIR="$FONTS" "$@"; }

pkgs=()
for t in "${TESTS[@]}"; do IFS='|' read -r _ p _ <<<"$t"; pkgs+=(-p "$p"); done
${LOCK_BUILD:-} bash -c "$(declare -f dev); REPO='$REPO' TARGET='$TARGET' FONTS='$FONTS'; dev cargo test --release --no-run ${pkgs[*]}"

for t in "${TESTS[@]}"; do
  IFS='|' read -r name pkg args <<<"$t"
  {
    echo "# cargo test --release -p $pkg $args"
    echo "# $(date -Is); load $(cut -d' ' -f1-3 /proc/loadavg); AIZOME_FONT_DIR=$(basename "$FONTS")"
    ${LOCK:-} bash -c "$(declare -f dev); REPO='$REPO' TARGET='$TARGET' FONTS='$FONTS'; dev cargo test --release -p $pkg $args" 2>&1 \
      | grep -v "^warning: Git tree"
  } >"$OUT/$name.txt"
  echo "$name done"
done
