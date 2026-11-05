#!/usr/bin/env bash
# Copy the batches' results into the dataset and summarise them.
#
#   tests/performance/media/collect.sh RESULTS_DIR [DATA_DIR]
#
# RESULTS_DIR holds one directory per batch (bench.py --out) and ttff/.
# Keeps per trial: trial.json, power.tsv.xz, igpu.tsv.xz and player.log.xz
# (the compositor's, D-Bus's and Xwayland's logs and smoke screenshots stay
# behind). Needs python3 on PATH.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../../.." && pwd)
results=$1
data=${2:-$repo/docs/performance/data/media}
mkdir -p "$data/batches"
batches=()
for b in "$results"/*/; do
  b=${b%/}
  name=$(basename "$b")
  [ -f "$b/results.jsonl" ] || continue
  dst="$data/batches/$name"
  # A batch marked exclude-from-summary is kept as data but left out of
  # the summaries (batch g: one round with the idle floor moving).
  [ -f "$b/exclude-from-summary" ] || batches+=("$dst")
  rm -rf "$dst"
  mkdir -p "$dst/trials"
  cp "$b/results.jsonl" "$b/batches.jsonl" "$b/programs.txt" "$dst/"
  [ -f "$b/exclude-from-summary" ] && cp "$b/exclude-from-summary" "$dst/"
  for t in "$b"/trials/*/; do
    t=${t%/}
    mkdir -p "$dst/trials/$(basename "$t")"
    for f in trial.json power.tsv.xz igpu.tsv.xz player.log.xz; do
      [ -f "$t/$f" ] && cp "$t/$f" "$dst/trials/$(basename "$t")/"
    done
  done
done
if [ -f "$results/ttff/ttff.jsonl" ]; then
  cp "$results/ttff/ttff.jsonl" "$data/ttff.jsonl"
fi
python3 -I "$here/summarize.py" "$data" "${batches[@]}" > /dev/null
du -sh "$data"
