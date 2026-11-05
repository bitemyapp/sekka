#!/usr/bin/env bash
# Summarise a headless.py --perf run (attribution only): for each phase's
# time window (record.json "perf_windows", CLOCK_MONOTONIC), the share of
# task-clock samples by process and shared object, and the top symbols.
# llvmpipe's rasteriser and shaders are JIT code: they show as [JIT] or
# [unknown] in the llvmpipe threads.
#
#   perf-summary.sh RUN_DIR PERF_BIN > perf-summary.txt
set -euo pipefail
run=$1
perf=$2
windows=$(tr -d ' \n' < "$run/record.json" | grep -o '"perf_windows":{[^}]*}' | grep -o '"[a-z]*":\[[0-9.,]*\]' | tr -d '"[]' | tr ':' ' ')
echo "$windows" | while read -r phase range; do
  [ -n "$phase" ] || continue
  echo "=== $phase ($range, CLOCK_MONOTONIC s): samples by process, thread and shared object"
  "$perf" report -i "$run/perf.data" --time "$range" --no-children --sort comm,dso --stdio -q --percent-limit 1.5 2>/dev/null \
    | sed 's/[[:space:]]*$//' | grep -v '^$' | head -30
  echo "=== $phase: top symbols"
  "$perf" report -i "$run/perf.data" --time "$range" --no-children --sort comm,dso,sym --stdio -q --percent-limit 0.8 2>/dev/null \
    | sed 's/[[:space:]]*$//' | grep -v '^$' | head -30
  echo
done
