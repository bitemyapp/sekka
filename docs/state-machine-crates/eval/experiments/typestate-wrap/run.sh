#!/usr/bin/env bash
# Measures typestate wrap variants: conformance, then gungraun instruction counts
# (baselines-only bench build), default profile and thin LTO.
#
#   SCRATCH=<dir> experiments/typestate-wrap/run.sh [--conformance-only] VARIANT...
#   SCRATCH=<dir> experiments/typestate-wrap/run.sh --probe PROBE VARIANT
#
# VARIANT names wrap-VARIANT.rs here (wrap-v0.rs is the measured
# impl-typestate/src/wrap.rs). Each variant in turn is copied over the
# workspace's impl-typestate/src/wrap.rs; the committed file is restored on
# exit. Logs go to $LOGS/VARIANT.log (default: logs/ here, which holds the
# recorded run; set LOGS elsewhere to keep it). parse.py VARIANT... prints the
# per-unit counts from those logs.
#
# --probe compiles and runs probes/PROBE.rs as an example of impl-typestate
# against VARIANT (the probes use wrap-borrowedfast.rs's with_typed), printing
# the compiler's output; the example is removed on exit.
#
# Variables: SCRATCH (required), a directory outside the repository;
# VALGRIND (default $SCRATCH/valgrind/bin) and RUNNER (default
# $SCRATCH/gungraun/bin/gungraun-runner) as in results/run.sh; CPUS, the CPUs
# the runs are pinned to (default 4-7,12-15, as run).
set -u
V=$(cd "$(dirname "$0")" && pwd)
SME=$(cd "$V/../.." && pwd)
: "${SCRATCH:?set SCRATCH to a scratch directory outside the repository}"
SCRATCH=$(realpath -m "$SCRATCH")
ISOLATE=$SME/isolate
VG=${VALGRIND:-$SCRATCH/valgrind/bin}
GR=${RUNNER:-$SCRATCH/gungraun/bin/gungraun-runner}
LOGS=${LOGS:-$V/logs}
CPUS=${CPUS:-4-7,12-15}
export CARGO_TARGET_DIR=$SCRATCH/t/smx
# The original wrapper, as isolate; isolate passes no other variable through,
# and the run used CARGO_BUILD_JOBS=4.
RUN=("$ISOLATE" default "$SCRATCH/isolate" env CARGO_BUILD_JOBS=4)

WRAP=$SME/impl-typestate/src/wrap.rs
EXAMPLES=$SME/impl-typestate/examples
mkdir -p "$SCRATCH" "$LOGS"
cp "$WRAP" "$SCRATCH/wrap.rs.committed"
# A plain cp gives the restored file a new mtime, so cargo rebuilds it.
cleanup() {
  cp "$SCRATCH/wrap.rs.committed" "$WRAP"
  [ -n "${probe:-}" ] && rm -f "$EXAMPLES/$probe.rs" && rmdir "$EXAMPLES" 2>/dev/null
  return 0
}
trap cleanup EXIT

conformance_only=
case "${1:-}" in
  --conformance-only) conformance_only=1; shift ;;
  --probe)
    probe=$2 variant=$3
    [ -e "$EXAMPLES/$probe.rs" ] && { echo "$EXAMPLES/$probe.rs exists" >&2; exit 1; }
    cp "$V/wrap-$variant.rs" "$WRAP"
    mkdir -p "$EXAMPLES"
    cp "$V/probes/$probe.rs" "$EXAMPLES/$probe.rs"
    "${RUN[@]}" bash -c "cd $SME && cargo run -q --locked -p impl-typestate --example $probe"
    exit $?
    ;;
esac

for variant in "$@"; do
  cp "$V/wrap-$variant.rs" "$WRAP"
  echo "=== $variant: conformance" | tee "$LOGS/$variant.log"
  taskset -c "$CPUS" "${RUN[@]}" bash -c "cd $SME && cargo test -q --locked -p impl-typestate --test spec -- wrap 2>&1 | tail -5" >> "$LOGS/$variant.log" 2>&1
  [ -n "$conformance_only" ] && continue
  for prof in default lto; do
    extra=""; [ $prof = lto ] && extra="CARGO_PROFILE_BENCH_LTO=thin CARGO_PROFILE_BENCH_CODEGEN_UNITS=1"
    echo "=== $variant: instructions ($prof)" >> "$LOGS/$variant.log"
    taskset -c "$CPUS" "${RUN[@]}" bash -c "cd $SME && PATH=$VG:\$PATH GUNGRAUN_RUNNER=$GR $extra GUNGRAUN_HOME=$CARGO_TARGET_DIR/gr-$variant-$prof cargo bench -q --locked -p bench --no-default-features --features iai --bench instructions -- '*wrap*' --save-summary=pretty-json 2>&1" >> "$LOGS/$variant.log" 2>&1
  done
done
echo done
