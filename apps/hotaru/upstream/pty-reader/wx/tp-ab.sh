#!/usr/bin/env bash
# A/B GUI throughput on wx's GPU: main's Hotaru (with the hash fix only)
# against the gather thread, alternating, all four corpora at 64 MiB.
set -uo pipefail
W=/home/callen/sekka-work/gui-tp
nix=/nix/var/nix/profiles/default/bin/nix
cd /home/callen/work/sekka-gui-throughput-base && $nix build "path:$PWD#hotaru" --out-link $W/hotaru-base > $W/base-build.log 2>&1 || { tail -5 $W/base-build.log; exit 1; }
echo "base $(readlink -f $W/hotaru-base)  gather $(readlink -f $W/hotaru-gather)"
c=/home/callen/work/hotaru-corpora
rm -rf $W/ab; mkdir -p $W/ab
for round in 1 2 3; do
  order="base gather"; [ $((round % 2)) = 0 ] && order="gather base"
  for v in $order; do
    python3 /home/callen/sekka-work/gpu-cmp/tools/frame-benchmark.py hotaru $W/hotaru-$v/bin/hotaru --renderer gpu --purpose qualification \
      --output $W/ab/$round-$v --font-dir /nix/store/b9vp4qd3gz9vmsl20grvd80hniirn5n5-aizome-fonts \
      --fallback-dir /nix/store/1rv68ixfzh8ki0jmanx7y846h2qdvzbg-hotaru-fonts --window-width 1349 --window-height 1077 \
      --cell-width 11 --cell-height 24 --keys 1 --frames 1 --control-seconds .1 --throughput-repeat 8 \
      --throughput $c/ascii.vt --throughput $c/styled.vt --throughput $c/unicode.vt --throughput $c/cursor.vt \
      --app-cpus 8-11 --compositor-cpus 12-13 --observer-cpus 14 --controller-cpus 15 --fixture-cpus 16 > /dev/null 2>&1 || echo "$round $v failed"
  done
done
python3 - $W/ab <<'PY'
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
