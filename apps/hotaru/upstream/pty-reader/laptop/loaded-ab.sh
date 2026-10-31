#!/usr/bin/env bash
# Laptop A/B: main's Hotaru (hash fix only) against the reader thread, on the
# Intel GPU, alternating, all four corpora at 64 MiB.
set -uo pipefail
G=/home/callen/work/gpu-cmp
W=$G/laptop-ab
T=/home/callen/work/sekka/.work/hotaru-gui-throughput/apps/hotaru/tools
c=$G/corpora
rm -rf $W; mkdir -p $W
for round in 1 2 3; do
  order="base gather"; [ $((round % 2)) = 0 ] && order="gather base"
  for v in $order; do
    echo "$(date +%T) round $round $v load $(cut -d' ' -f1 /proc/loadavg)"
    python3 $T/frame-benchmark.py hotaru $G/hotaru-$v/bin/hotaru --renderer gpu --purpose qualification \
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
        rows.setdefault((Path(t["corpus"]).stem, variant), []).append((t["consume_ns"] / 1e6, t["application_cpu_ms"]))
for corpus in ("ascii", "styled", "unicode", "cursor"):
    out = []
    for v in ("base", "gather"):
        r = rows.get((corpus, v), [])
        if r:
            out.append(f"{v} {statistics.median(x for x, _ in r):7.1f} ms ({64 / statistics.median(x for x, _ in r) * 1000:6.1f} MiB/s, cpu {statistics.median(c for _, c in r):5.0f} ms) runs {[round(x) for x, _ in r]}")
    print(f"{corpus:8}", " | ".join(out))
PY
