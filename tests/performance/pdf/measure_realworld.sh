#!/usr/bin/env bash
# The real-world documents (data/pdf/realworld.tsv, downloaded and listed one
# path a line in $WORK/realworld.txt): the first 20 pages of each, five rounds,
# the engines' order rotated, four files at a time on cores 2-5. No power meter.
# Run under lock-measure, inside shell.nix.
here=$(cd "$(dirname "$0")" && pwd)
WORK=${WORK:-${TMPDIR:-/tmp}/aizome-perf/pdf}
cd "$WORK"
export PDFBENCH_PAGES=20
engines=(pdfium hayro-fork hayro-upstream mupdf poppler-splash poppler-cairo)
for round in 1 2 3 4 5; do
  echo "$(date -Is) realworld round $round load $(cut -d' ' -f1-3 /proc/loadavg)" >> results/rounds.log
  n=${#engines[@]}
  for i in $(seq 0 $((n-1))); do
    e=${engines[$(( (i + round) % n ))]}
    python3 -I "$here/run_corpus.py" --engine "$e" --bin "bin/pdfbench-$e" --list realworld.txt --tag "w$round" \
      --pages-tsv results/realworld-pages.tsv --files-tsv results/realworld-files.tsv --cores 2,3,4,5
  done
done
echo "$(date -Is) realworld end load $(cut -d' ' -f1-3 /proc/loadavg)" >> results/rounds.log
