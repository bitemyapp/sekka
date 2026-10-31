#!/usr/bin/env bash
# Profile Ghostty during GUI throughput on wx's GPU, the same way as Hotaru.
set -uo pipefail
mode=${1:-perf}; corpus=${CORPUS:-ascii}
W=/dev/shm/gui-tp-prof/ghostty-$mode
rm -rf $W; mkdir -p $W
g=/nix/store/rvbnq9b2j16127m46g7ia1vv9zw5cy03-ghostty-1.4.1-dev+6ba144158577d033ccb6ed4b574e4013957c9161-nix/bin/ghostty
cat > $W/ghostty <<WRAP
#!/usr/bin/env bash
case $mode in
  perf) exec /home/callen/sekka-work/perf-tool/bin/perf record -q -F 2999 -o $W/perf.data -- $g "\$@" ;;
  strace) exec strace -f -c -o $W/strace.txt $g "\$@" ;;
esac
WRAP
chmod +x $W/ghostty
python3 /home/callen/sekka-work/gpu-cmp/tools/frame-benchmark.py ghostty $W/ghostty --renderer gpu --purpose qualification --output $W/run \
  --app-env /home/callen/sekka-work/gpu-cmp/tools/wx-ghostty-env.json --ghostty-cell-height=-1 \
  --font-dir /nix/store/b9vp4qd3gz9vmsl20grvd80hniirn5n5-aizome-fonts --fallback-dir /nix/store/1rv68ixfzh8ki0jmanx7y846h2qdvzbg-hotaru-fonts \
  --window-width 1349 --window-height 1077 --cell-width 11 --cell-height 24 --keys 1 --frames 1 --control-seconds .1 \
  --throughput /home/callen/work/hotaru-corpora/$corpus.vt --throughput-repeat 8 \
  --app-cpus 8-11 --compositor-cpus 12-13 --observer-cpus 14 --controller-cpus 15 --fixture-cpus 16 2>&1 | tail -1
python3 -c "
import json,sys; r=json.load(open(sys.argv[1]))
for t in r.get('throughput', []): print(t['corpus'].split('/')[-1], round(t['consume_ns']/1e6,1), 'ms', round(t['mib_per_second'],1), 'MiB/s')
" $W/run/result.json
