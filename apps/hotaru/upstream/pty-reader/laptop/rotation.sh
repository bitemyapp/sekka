#!/usr/bin/env bash
# Laptop: main's Hotaru, the reader thread and Ghostty, rotating, on the Intel GPU.
set -uo pipefail
G=/home/callen/work/gpu-cmp
W=$G/laptop-abc
T=/home/callen/work/sekka/.work/hotaru-gui-throughput/apps/hotaru/tools
c=$G/corpora
rm -rf $W; mkdir -p $W
orders=("base gather ghostty" "gather ghostty base" "ghostty base gather" "base ghostty gather")
for round in 1 2 3 4; do
  for v in ${orders[$((round - 1))]}; do
    echo "$(date +%T) round $round $v load $(cut -d' ' -f1 /proc/loadavg)"
    if [ $v = ghostty ]; then app=(ghostty $G/ghostty/bin/ghostty --ghostty-cell-height=-1); else app=(hotaru $G/hotaru-$v/bin/hotaru); fi
    python3 $T/frame-benchmark.py "${app[@]}" --renderer gpu --purpose qualification \
      --output $W/$round-$v --font-dir /nix/store/b9vp4qd3gz9vmsl20grvd80hniirn5n5-aizome-fonts \
      --fallback-dir /nix/store/1rv68ixfzh8ki0jmanx7y846h2qdvzbg-hotaru-fonts --window-width 1349 --window-height 1077 \
      --cell-width 11 --cell-height 24 --keys 1 --frames 1 --control-seconds .1 --throughput-repeat 8 \
      --throughput $c/ascii.vt --throughput $c/styled.vt --throughput $c/unicode.vt --throughput $c/cursor.vt \
      --protocol-xml screencopy=$SCREENCOPY_XML --protocol-xml virtual-keyboard=$VIRTUAL_KEYBOARD_XML \
      --app-cpus 0-2 --compositor-cpus 3 --observer-cpus 4 --controller-cpus 5 --fixture-cpus 6 > $W/$round-$v.log 2>&1 || echo "$round $v failed: $(tail -3 $W/$round-$v.log)"
  done
done
python3 - $W <<'PY'
import json, statistics, sys
from pathlib import Path
rows = {}
for path in sorted(Path(sys.argv[1]).glob("*/result.json")):
    variant = path.parent.name.split("-", 1)[1]
    for t in json.loads(path.read_text()).get("throughput", []):
        rows.setdefault((Path(t["corpus"]).stem, variant), []).append((t["consume_ns"] / 1e6, t.get("application_cpu_ms", 0)))
for corpus in ("ascii", "styled", "unicode", "cursor"):
    out = []
    for v in ("base", "gather", "ghostty"):
        r = rows.get((corpus, v), [])
        if r:
            m = statistics.median(x for x, _ in r)
            out.append(f"{v} {m:6.0f} ms ({64 / m * 1000:5.0f} MiB/s, cpu {statistics.median(c for _, c in r):4.0f}) {[round(x) for x, _ in r]}")
    print(f"{corpus:8}", " | ".join(out))
PY
