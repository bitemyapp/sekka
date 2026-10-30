#!/usr/bin/env bash
# GUI throughput on wx's NVIDIA GPU: four corpora, 64 MiB each, four alternating pairs.
set -uo pipefail
src=/home/callen/sekka-work/gpu-cmp/tools
W=/home/callen/sekka-work/gpu-cmp/wx
c=/home/callen/work/hotaru-corpora
rm -rf $W/throughput
python3 $src/frame-matrix.py --output $W/throughput --calibration $W/calibration \
  --font-dir /nix/store/b9vp4qd3gz9vmsl20grvd80hniirn5n5-aizome-fonts --fallback-dir /nix/store/1rv68ixfzh8ki0jmanx7y846h2qdvzbg-hotaru-fonts \
  --renderer gpu --ghostty-env $src/wx-ghostty-env.json --phase throughput --rounds 4 --quiet-wait 120 \
  --corpus $c/ascii.vt --corpus $c/styled.vt --corpus $c/unicode.vt --corpus $c/cursor.vt --throughput-repeat 8 \
  --app-cpus 8-11 --compositor-cpus 12-13 --observer-cpus 14 --controller-cpus 15 --fixture-cpus 16 | tail -1
