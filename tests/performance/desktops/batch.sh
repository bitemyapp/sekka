#!/usr/bin/env bash
# Run queued trials while holding the measurement lock once, up to a time
# budget, so each turn at the shared lock does several boots. QUEUE holds
# one trial per line: "VARIANT TAG [bench.py trial options]"; a trial is
# removed from it when it starts. A trial is started only if the minutes
# it is expected to take (EST, default 13) still fit in BUDGET (default 44).
#
#   lock-measure batch.sh QUEUE [BUDGET] [EST]
set -uo pipefail
here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
queue=$1 budget=${2:-44} est=${3:-13}
start=$(date +%s)
while true; do
  line=$(head -n 1 "$queue")
  [ -n "$line" ] || break
  elapsed=$((($(date +%s) - start) / 60))
  # Shorter trials (memory and idle only) declare their own estimate.
  need=$est
  case $line in baseline*|*"--phases hostidle,idle"*) need=7 ;; esac
  if [ $((elapsed + need)) -gt "$budget" ]; then
    echo "=== budget: $elapsed min used, next needs $need"
    break
  fi
  sed -i 1d "$queue"
  # shellcheck disable=SC2086
  set -- $line
  echo "=== trial $line $(date +%T)"
  "$here/run.sh" trial "$@"
  echo "=== trial $1 $2 exit $? $(date +%T)"
done
