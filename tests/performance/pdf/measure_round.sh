#!/usr/bin/env bash
# One measured round: an idle baseline, every engine's full pass over the
# corpus (in the order given) under the PPT power meter, then another idle
# baseline. Run under tests/performance/lock-measure, inside shell.nix:
#   lock-measure nix-shell tests/performance/pdf/shell.nix --run \
#     'tests/performance/pdf/measure_round.sh 1 pdfium hayro-fork ...'
# Environment: WORK (default $TMPDIR/aizome-perf/pdf) holds bin/, the file
# list and results/; LIST is the file list (corpus.txt); PREFIX names the
# rounds (r: r1, r2...) in results; PAGES and FILES are the TSVs appended to
# (pages.tsv, files.tsv); POWERMETER is the meter (skipped if it isn't there);
# CORES the cores to pin to.
set -uo pipefail
here=$(cd "$(dirname "$0")" && pwd)
WORK=${WORK:-${TMPDIR:-/tmp}/aizome-perf/pdf}
POWERMETER=${POWERMETER:-}
CORES=${CORES:-2,3,4,5}
IDLE_S=${IDLE_S:-20}
LIST=${LIST:-corpus.txt}
PREFIX=${PREFIX:-r}
PAGES=${PAGES:-pages.tsv}
FILES=${FILES:-files.tsv}
round=$1; shift
cd "$WORK"
mkdir -p results/power
log=results/rounds.log
note() { echo "$(date -Is) round $PREFIX$round $* load $(cut -d' ' -f1-3 /proc/loadavg)" | tee -a "$log"; }
# meter NAME CMD...: CMD under the power meter, its summary in results/power/NAME.json
# (or just CMD where there is no meter).
meter() {
  local name=$1; shift
  if [ -x "$POWERMETER" ]; then
    "$POWERMETER" -j "results/power/$name.json" -o "results/power/$name.tsv" -- "$@" > /dev/null
  else
    "$@" > /dev/null
  fi
}

# Wait (up to 10 minutes) for a quiet machine.
for _ in $(seq 60); do
  awk -v l="$(cut -d' ' -f1 /proc/loadavg)" 'BEGIN { exit !(l < 1.5) }' && break
  sleep 10
done
note "start ($LIST)"
meter "$PREFIX$round-idle-a" sleep "$IDLE_S"
for e in "$@"; do
  note "$e start"
  meter "$PREFIX$round-$e" python3 -I "$here/run_corpus.py" --engine "$e" --bin "bin/pdfbench-$e" --list "$LIST" \
    --tag "$PREFIX$round" --pages-tsv "results/$PAGES" --files-tsv "results/$FILES" --cores "$CORES"
  note "$e end"
done
meter "$PREFIX$round-idle-b" sleep "$IDLE_S"
note "end"
