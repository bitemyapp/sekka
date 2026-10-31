#!/usr/bin/env bash
# Profile Hotaru (cargo build with line tables) during GUI throughput on wx's GPU.
set -uo pipefail
mode=${1:-perf}
W=/dev/shm/gui-tp-prof/$mode
rm -rf $W; mkdir -p $W
cat > $W/hotaru <<WRAP
#!/usr/bin/env bash
export LD_LIBRARY_PATH="\$(cat /dev/shm/gui-tp-ldpath)"
set -a; . /dev/shm/gui-tp-env; set +a
case $mode in
  perf) exec /home/callen/sekka-work/perf-tool/bin/perf record -q -F 2999 -o $W/perf.data -- /dev/shm/gui-tp-target/release-lto/aizome-hotaru "\$@" ;;
  strace) exec strace -f -c -o $W/strace.txt /dev/shm/gui-tp-target/release-lto/aizome-hotaru "\$@" ;;
  plain) exec /dev/shm/gui-tp-target/release-lto/aizome-hotaru "\$@" ;;
esac
WRAP
chmod +x $W/hotaru
c=/home/callen/work/hotaru-corpora
python3 /home/callen/sekka-work/gpu-cmp/tools/frame-benchmark.py hotaru $W/hotaru --renderer gpu --purpose qualification --output $W/run \
  --font-dir /nix/store/b9vp4qd3gz9vmsl20grvd80hniirn5n5-aizome-fonts --fallback-dir /nix/store/1rv68ixfzh8ki0jmanx7y846h2qdvzbg-hotaru-fonts \
  --window-width 1349 --window-height 1077 --cell-width 11 --cell-height 24 --keys 1 --frames 1 --control-seconds .1 \
  --throughput $c/${CORPUS:-ascii}.vt --throughput-repeat 8 \
  --app-cpus 8-11 --compositor-cpus 12-13 --observer-cpus 14 --controller-cpus 15 --fixture-cpus 16 2>&1 | tail -1
python3 -c "
import json,sys; r=json.load(open(sys.argv[1]))
for t in r.get('throughput', []): print(t['corpus'].split('/')[-1], round(t['consume_ns']/1e6,1), 'ms', round(t['mib_per_second'],1), 'MiB/s', 'app cpu', t['application_cpu_ms'])
" $W/run/result.json
