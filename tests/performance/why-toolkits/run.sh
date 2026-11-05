#!/usr/bin/env bash
# Every measured run behind docs/performance/why-toolkits.md, in chunks of
# under 45 minutes, each chunk inside the shared measurement lock.
#
#   tests/performance/why-toolkits/run.sh CHUNK [ROUNDS]
#
# CHUNK: typing-lat typing-lat2 typing-trace scroll 4k terms apps native-type
#        native-scroll native-apps perf
# LOCK (optional) is a command each chunk runs under (here
# tests/performance/lock-measure). TK_WORK (default /tmp/claude-1000/tk)
# holds the pins (roots/), tools, fixtures and runs; see README in the data
# directory for building them.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
W=${TK_WORK:-/tmp/claude-1000/tk}
py=$W/roots/python3/bin/python3
chunk=${1:?chunk}
rounds=${2:-3}
export PATH=$W/roots/python3/bin:/run/current-system/sw/bin
S() { ${LOCK:-} "$py" "$here/run_sway.py" "$W/runs/sway" "$rounds" "$@"; }
N() { ${LOCK:-} "$py" "$here/run_native.py" "$W/runs/native" "$rounds" "$@"; }
case $chunk in
typing-lat) S lat-small,lat-large,lat-fullcopy ;;
typing-lat2) S lat240-small,lat-idle-small ;;
typing-trace) S trace-small,trace-large,cost-small,cost-large ;;
scroll) S scroll-paced,scroll-flat,scroll-page,resize,blink ;;
4k) S 4k-lat,4k-cost,4k-flat,4k-page,4k-ceiling,4k-blink ;;
# Hotaru and foot: the terminal comparison's extra frame. A raw-mode program
# answering each key in one write (as terminals.md's did) or two, and cat.
terms) TK_EXTRA="--args ''" S lat-small,lat-fullcopy,trace-small hotaru,foot,hotaru-two,foot-two,hotaru-cat,foot-cat ;;
# Real apps: the large file typed at its start; a folder of 10,000 files
# scrolled by wheel notches.
apps)
  ${LOCK:-} "$py" "$here/run_apps.py" "$W/runs/apps" "$rounds" ;;
native-type) N keys-small,keys-large ;;
native-scroll) N scroll-paced,scroll-flat,scroll-page ;;
native-apps) N app-keys aizome:sumi,gnome:gte,kde:kwrite ;;
# Attribution: perf record and perf stat, one round, every matched program.
perf)
  for mode in record stat; do
    TK_EXTRA="--perf $mode" ${LOCK:-} "$py" "$here/run_sway.py" "$W/runs/perf-$mode" 1 cost-small,scroll-flat,4k-page
  done ;;
*) echo "unknown chunk $chunk" >&2; exit 2 ;;
esac
