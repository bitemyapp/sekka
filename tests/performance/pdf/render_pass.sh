#!/usr/bin/env bash
# The untimed pass that writes every page of the corpus for the pixel
# comparison: render_pass.sh ENGINE... (TAG names the result files, default
# "render"). Six processes at a time on cores 2-7; each engine's pages take
# 5-7 GB as PPM under $WORK/render/<engine>.
here=$(cd "$(dirname "$0")" && pwd)
WORK=${WORK:-${TMPDIR:-/tmp}/aizome-perf/pdf}
cd "$WORK"
for e in "$@"; do
  rm -rf "render/$e"; mkdir -p "render/$e"
  python3 -I "$here/run_corpus.py" --engine "$e" --bin "bin/pdfbench-$e" --list corpus.txt --tag "${TAG:-render}" \
    --pages-tsv "results/${TAG:-render}-pages.tsv" --files-tsv "results/${TAG:-render}-files.tsv" \
    --cores 2,3,4,5,6,7 --render-dir "render/$e"
done
