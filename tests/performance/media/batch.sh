#!/usr/bin/env bash
# One measurement lock hold: a plan, then optionally the first-frame timing.
#
#   tests/performance/media/batch.sh PLAN.json OUT_DIR [TTFF_OUT_DIR]
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
perf=${PERF_DIR:-${TMPDIR:-/tmp}/aizome-perf}
plan=$1
out=$2
ttff=${3:-}
inner() {
  LOCK=none "$here/run.sh" "$plan" "$out" --max-minutes 38
  if [ -n "$ttff" ]; then
    LOCK=none "$here/run.sh" ttff "${CLIPS:-$perf/media/clips}/pattern-1080p24-h264.mp4" "$ttff" --reps 5
  fi
}
export -f inner
export here plan out ttff perf
exec "$here/../lock-measure" bash -c inner
