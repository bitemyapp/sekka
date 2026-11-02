#!/usr/bin/env bash
# Copies the results in $WORK into docs/performance/data/pdf, compressed.
# Run inside shell.nix after the rounds, render passes, suites and analyses.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
WORK=${WORK:-${TMPDIR:-/tmp}/aizome-perf/pdf}
R=$WORK/results
D=$here/../../../docs/performance/data/pdf
mkdir -p "$D/power" "$D/conformance"
# The first energy round passed over the common files once, a pilot; its power
# files were renamed p1-*, and so are its rows here.
sed -i 's/^e1\t/p1\t/' "$R/pages-common.tsv" "$R/files-common.tsv"
python3 -I "$here/analyze.py" "$R" "$D" > /dev/null
python3 -I "$here/analyze_extra.py" "$R" "$D"
xz -9 -f -T1 "$D/page-medians.csv"
[ -f "$D/page-medians-pdfjs.csv" ] && xz -9 -f -T1 "$D/page-medians-pdfjs.csv"
for f in pages files agreement agreement-fork-vs-upstream agreement-pdfjs render-files pages-common files-common \
         pages-pdfjs files-pdfjs realworld-pages realworld-files; do
  [ -f "$R/$f.tsv" ] && xz -9 -T1 -c "$R/$f.tsv" > "$D/$f.tsv.xz"
done
[ -f "$D/shiori-corpus.tsv" ] && xz -9 -f -T1 "$D/shiori-corpus.tsv"
cp "$R/rounds.log" "$D/rounds.log"
sed 's|.*/||' "$WORK/corpus-common.txt" | sort -u > "$D/common-files.txt"
cp "$R"/power/*.json "$D/power/"
( echo -e "round\tsubject\tseconds\twatts"
  for f in "$R"/power/*.tsv; do
    b=$(basename "$f" .tsv); awk -v r="${b%%-*}" -v s="${b#*-}" -F'\t' '{print r "\t" s "\t" $1 "\t" $2}' "$f"
  done ) | xz -9 -T1 > "$D/power/power-samples.tsv.xz"
for b in fork upstream base; do
  xz -9 -T1 -c "$R/suite-hayro-$b.jsonl" > "$D/conformance/hayro-suite-$b.jsonl.xz"
  cp "$R/suite-$b-summary.json" "$D/conformance/hayro-suite-$b-summary.json"
  xz -9 -T1 -c "$R/suite-$b-failures.tsv" > "$D/conformance/hayro-suite-$b-failures.tsv.xz"
done
cp "$WORK/pixel/pixel-summary.json" "$D/conformance/pdfium-pixel-summary.json"
xz -9 -T1 -c "$WORK/pixel/pixel-results.json" > "$D/conformance/pdfium-pixel-results.json.xz"
cp "$WORK/pixel/pixel-tests.tsv" "$D/conformance/pdfium-pixel-tests.tsv"
du -sh "$D"
