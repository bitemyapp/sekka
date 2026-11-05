#!/usr/bin/env bash
# Shiori's own path (desktop/aizome/apps/shiori/examples/corpus.rs) over the
# corpus, three runs, four files at a time, the process and its workers pinned
# to cores 2-5. $SHIORI_TARGET is the cargo target directory it was built in
# (cargo build --release -p aizome-shiori --bin aizome-shiori --example corpus).
# Run under lock-measure.
WORK=${WORK:-${TMPDIR:-/tmp}/aizome-perf/pdf}
SHIORI_TARGET=${SHIORI_TARGET:-$WORK/target-shiori}
cd "$WORK"
for _ in $(seq 60); do awk -v l="$(cut -d' ' -f1 /proc/loadavg)" 'BEGIN { exit !(l < 1.5) }' && break; sleep 10; done
for run in 1 2 3; do
  echo "$(date -Is) shiori run $run start load $(cut -d' ' -f1-3 /proc/loadavg)" >> results/rounds.log
  taskset -c 2,3,4,5 "$SHIORI_TARGET/release/examples/corpus" src/pdfjs/test/pdfs 4 \
    > "results/shiori-corpus-run$run.tsv" 2> "results/shiori-corpus-run$run.err"
  echo "$(date -Is) shiori run $run end load $(cut -d' ' -f1-3 /proc/loadavg)" >> results/rounds.log
done
