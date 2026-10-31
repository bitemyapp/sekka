#!/usr/bin/env bash
# Private-Sway suites against the patched package, then a keyboard-latency A/B.
set -uo pipefail
W=/home/callen/sekka-work/gui-tp
mirror=/home/callen/work/sekka-gui-throughput
result=$W/hotaru-gather
mkdir -p $W/suites
(
  exec 9>/home/callen/.cache/hotaru-workstation.lock
  flock -x -w 1800 9
  cd $mirror/desktop/aizome/apps/hotaru/tests/wayland
  for suite in selections settings smoke fonts search shell links auto_links cursor pixel_scroll sprites; do
    args=("$W/suites/$suite")
    [[ $suite == sprites ]] && args=(/home/callen/work/hotaru-sprites-geometric-20261028/oracle-expanded/libsprites-fast.so "${args[@]}")
    rm -rf "$W/suites/$suite"
    if env -u LD_LIBRARY_PATH PATH=/usr/bin:/bin python3 $suite.py $result/bin/hotaru "${args[@]}" > "$W/suites/$suite.log" 2>&1; then
      echo "$suite: passed"
    else
      echo "$suite: FAILED"; tail -8 "$W/suites/$suite.log"
    fi
  done
)
rm -rf $W/latency; mkdir -p $W/latency
for round in 1 2; do
  order="base gather"; [ $round = 2 ] && order="gather base"
  for v in $order; do
    python3 /home/callen/sekka-work/gpu-cmp/tools/frame-benchmark.py hotaru $W/hotaru-$v/bin/hotaru --renderer gpu --purpose qualification \
      --output $W/latency/$round-$v --font-dir /nix/store/b9vp4qd3gz9vmsl20grvd80hniirn5n5-aizome-fonts \
      --fallback-dir /nix/store/1rv68ixfzh8ki0jmanx7y846h2qdvzbg-hotaru-fonts --window-width 1349 --window-height 1077 \
      --cell-width 11 --cell-height 24 --keys 200 --frames 120 --control-seconds 3 \
      --app-cpus 8-11 --compositor-cpus 12-13 --observer-cpus 14 --controller-cpus 15 --fixture-cpus 16 > /dev/null 2>&1 || echo "$round $v failed"
  done
done
python3 - $W/latency <<'PY'
import json, statistics, sys
from pathlib import Path
by = {}
for p in sorted(Path(sys.argv[1]).glob("*/result.json")):
    r = json.loads(p.read_text()); v = p.parent.name.split("-", 1)[1]
    by.setdefault(v, {"keys": [], "streams": {}})
    by[v]["keys"] += [k["inject_to_capture_ns"] / 1e6 for k in r["keyboard"]]
    for s in r["streams"]:
        by[v]["streams"].setdefault(f"{s['kind']} {s['producer_hz']}", []).append(s["observed_unique"])
for v, d in by.items():
    k = sorted(d["keys"]); q = statistics.quantiles(k, n=100)
    print(f"{v:7} keys n={len(k)} p50 {statistics.median(k):.1f} p95 {q[94]:.1f} p99 {q[98]:.1f} max {k[-1]:.1f} ms; seen {{{', '.join(f'{n}: {s}' for n, s in d['streams'].items() if not n.endswith(' 0'))}}}")
PY
