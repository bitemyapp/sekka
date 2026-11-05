#!/usr/bin/env bash
# Summaries and compressed raw runs into docs/performance/data/why-toolkits.
#
#   tests/performance/why-toolkits/pack.sh
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../../.." && pwd)
W=${TK_WORK:-/tmp/claude-1000/tk}
py=$W/roots/python3/bin/python3
out=$repo/docs/performance/data/why-toolkits
mkdir -p "$out/perf"
for set in sway apps native; do
  [ -d "$W/runs/$set" ] || continue
  "$py" "$here/analyze.py" "$W/runs/$set" "$out/summary-$set.json"
  "$py" "$here/report.py" "$out/summary-$set.json" > "$out/tables-$set.md"
  # Raw runs, without perf's own data files (perf/ holds their summaries).
  tar -C "$W/runs" --exclude='*.data' --exclude='*.data.old' -cf - "$set" | xz -9e -T4 > "$out/runs-$set.tar.xz"
done
for mode in record stat; do
  d=$W/runs/perf-$mode
  [ -d "$d" ] || continue
  tar -C "$W/runs" --exclude='*.data' --exclude='*.data.old' -cf - "perf-$mode" | xz -9e -T4 > "$out/runs-perf-$mode.tar.xz"
done
ls -la "$out"
