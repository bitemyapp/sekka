#!/usr/bin/env bash
# Phase 3b: measure every implementation in the workspace (this script's
# parent directory, docs/state-machine-crates/eval), writing raw outputs to
# results/raw/. These are the commands as they were run, with the original
# run's scratch paths replaced by the variables below.
#
#   SCRATCH=<dir> results/run.sh [step ...]    default: every step, in this order:
#   env conformance proptest proptest-cases compile-fail fmt loc sizes counts
#   compile smoke instructions instructions-solo instructions-lto criterion criterion-lto
#   perf-check tables
#
# In the evaluation of f603466 (raw/steps.log has the times) the steps ran as
# `env`, `conformance fmt`, `proptest compile-fail loc sizes counts`,
# `proptest-cases`, `compile smoke instructions criterion`; instructions-solo
# and instructions-lto were started during criterion run 1 and took the lock
# between its groups; perf-check ran after run 2 and again once the LTO
# criterion binary was built; then criterion-lto and tables. Two fixes made
# during the run are in the steps below: proptest-cases' grep (rerun), and
# instructions copying whole gungraun dirs (copied again by hand, same files).
# When the results were copied into the repository, the criterion and
# gungraun directories in raw/ were folded into JSON Lines
# (tools/consolidate.py; see raw/README.md), and the steps below now write
# those files instead of copying the directories.
#
# Steps that measure (compile, smoke, instructions*, criterion*, perf-check)
# hold the host's benchmark lock; criterion, smoke and perf-check run pinned
# to CPU 3 (its SMT sibling is 11). Nothing here changes governors or other
# system settings.
#
# Variables (from the environment):
#   SCRATCH   required: a directory outside the repository for isolate's HOME,
#             XDG and temporary directories and every target directory.
#   VALGRIND  valgrind's bin directory (default $SCRATCH/valgrind/bin), from
#             `nix build --inputs-from <repository root> nixpkgs#valgrind --out-link $SCRATCH/valgrind`.
#             The run used valgrind 3.27.1,
#             /nix/store/4298593522xgrr28874w20cvvs742r51-valgrind-3.27.1.
#   RUNNER    gungraun-runner (default $SCRATCH/gungraun/bin/gungraun-runner), from
#             `cargo install gungraun-runner --version 0.20.0 --locked --root $SCRATCH/gungraun`.
#   LOCK      the benchmark lock (default ~/.cache/hotaru-runnymede-benchmark.lock).
#   PERF      perf, the system's (default: perf on PATH). The run used
#             /nix/store/bw8frr0nk26m740a2mwaw5yi06mgxakb-perf-linux-7.3.1/bin/perf.
set -uo pipefail

: "${SCRATCH:?set SCRATCH to a scratch directory outside the repository}"
SCRATCH=$(realpath -m "$SCRATCH")
SME=$(cd "$(dirname "$0")/.." && pwd)
OUT=$SME/results
RAW=$OUT/raw
ISOLATE=$SME/isolate
export CARGO_TARGET_DIR=$SCRATCH/t/sme-eval
LOCK=${LOCK:-$HOME/.cache/hotaru-runnymede-benchmark.lock}
VALGRIND=${VALGRIND:-$SCRATCH/valgrind/bin}
RUNNER=${RUNNER:-$SCRATCH/gungraun/bin/gungraun-runner}
PERF=${PERF:-perf}
IMPLS="implicit enum typestate statig smlang sml finny fsmentry statum state-machines rust-fsm"
BENCH_GROUPS="pointer-stream wrap-80 wrap-200 replay-pointer replay-wrap replay-polkit replay-transfer replay-lock replay-install"
# The callgrind profiles raw/ keeps (results.md and the report cite them), as
# <build>:<bench>.<impl>; every other profile is left in the target dir.
CALLGRIND_KEEP="all:pointer_stream.enum_ stub:pointer_stream.enum_ all:wrap_80.sml lto-all:wrap_80.sml lto-all:wrap_80.enum_ lto-all:wrap_80.typestate"
mkdir -p "$RAW"

# iso COMMAND...: COMMAND in the root flake's default dev shell through
# isolate (an empty environment; HOME, XDG and TMPDIR under
# $SCRATCH/isolate; the target dir from CARGO_TARGET_DIR). isolate passes no
# other variable through, so CARGO_BUILD_JOBS=8, which the original wrapper
# set, is set here. python3 is the dev shell's (the run's was
# /nix/store/xahxrjs30p3mw0a42bqg9ikgkikskw8m-python3-3.14.8).
iso() { "$ISOLATE" default "$SCRATCH/isolate" env CARGO_BUILD_JOBS=8 "$@"; }
export -f iso
export ISOLATE SCRATCH
# A command in the wrapper, from the workspace, with this evaluation's target dir.
w() { iso bash -c "cd $SME && $1"; }
# Appends "<name> <exit status>" to raw/exit-codes.txt.
status() { echo "$1 $2" >> "$RAW/exit-codes.txt"; }
# Appends the load average, the top 10 processes by CPU, and the /proc/stat
# lines of the pinned CPU and its sibling to a log.
load() {
  { echo "== $1 $(date -Is)"; cat /proc/loadavg; ps -eo pid,pcpu,comm --sort=-pcpu | head; grep -E '^cpu(3|11) ' /proc/stat; } >> "$2"
}
export -f load
# locked LABEL LOG COMMAND...: COMMAND under the benchmark lock, the load
# logged just before and after it (inside the lock).
locked() {
  local label=$1 log=$2
  shift 2
  flock -o "$LOCK" bash -c 'l=$1 g=$2; shift 2; load "$l: before" "$g"; "$@"; rc=$?; load "$l: after" "$g"; exit $rc' _ "$label" "$log" "$@"
}
# Waits while the 1-minute load average exceeds 4: polls every 3 minutes, up
# to an hour, and logs every poll.
wait_load() {
  local log=$1 i one
  for i in $(seq 0 20); do
    read -r one _ < /proc/loadavg
    if awk "BEGIN { exit !($one <= 4) }"; then
      echo "$(date -Is) load $one <= 4: starting (after $i polls)" >> "$log"
      return
    fi
    echo "$(date -Is) load $one > 4: waiting (poll $i)" >> "$log"
    [ "$i" -lt 20 ] && sleep 180
  done
  echo "$(date -Is) still above 4 after an hour: starting anyway" >> "$log"
}
# consolidate KIND DIR OUT: a criterion or gungraun output directory as
# JSON Lines (tools/consolidate.py).
consolidate() { iso python3 -I "$OUT/tools/consolidate.py" "$1" "$2" > "$3"; }
# keep_callgrind BUILD DIR: copies BUILD's profiles named in CALLGRIND_KEEP
# from a gungraun directory to raw/callgrind/<build>.<bench>.<impl>.out.
keep_callgrind() {
  local b=$1 dir=$2 k x
  mkdir -p "$RAW/callgrind"
  for k in $CALLGRIND_KEEP; do
    [ "${k%%:*}" = "$b" ] || continue
    x=${k#*:}
    cp "$dir/bench/instructions/machines/$x/callgrind.$x.out" "$RAW/callgrind/$b.$x.out"
  done
}

step_env() {
  {
    echo "date: $(date -Is)"
    # The run recorded the scratch repository's commit (f603466); here it is
    # the repository commit holding this copy.
    echo "commit: $(git -C "$SME" rev-parse HEAD) ($(git -C "$SME" log -1 --format=%s))"
    echo "git status --short (excluding results/):"
    git -C "$SME" status --short -- . ':!results' | grep -v '^?? results/'
    echo
    w 'rustc -vV; cargo -V; echo CARGO_BUILD_JOBS=$CARGO_BUILD_JOBS CARGO_INCREMENTAL=$CARGO_INCREMENTAL CARGO_PROFILE_DEV_DEBUG=$CARGO_PROFILE_DEV_DEBUG CARGO_TARGET_DIR=$CARGO_TARGET_DIR'
    echo
    uname -a
    echo
    lscpu
    echo
    grep -m1 'model name' /proc/cpuinfo
    for f in /sys/devices/system/cpu/cpu{3,11}/cpufreq/scaling_governor /sys/devices/system/cpu/cpufreq/boost /sys/devices/system/cpu/cpu3/topology/thread_siblings_list; do
      [ -r "$f" ] && echo "$f: $(cat "$f")"
    done
    echo
    "$VALGRIND/valgrind" --version
    "$RUNNER" --version
    cat /proc/loadavg
  } > "$RAW/env.txt" 2>&1
}

step_conformance() {
  w "python3 metrics/sme.py conformance" > "$RAW/conformance.txt" 2>&1
  status conformance $?
  cp "$SME/metrics/out/conformance.jsonl" "$RAW/conformance.jsonl"
  w "python3 metrics/sme.py conformance --fixed" > "$RAW/conformance-fixed.txt" 2>&1
  status conformance-fixed $?
  cp "$SME/metrics/out/conformance.jsonl" "$RAW/conformance-fixed.jsonl"
}

step_proptest() {
  local n
  for n in $IMPLS; do
    w "SME_PROPTEST_CASES=20000 cargo test --locked --release -p impl-$n" > "$RAW/proptest-$n.txt" 2>&1
    status "proptest-$n" $?
  done
}

# The case counts the differential tests ran (their eprintln is captured on
# success above): the same tests again, with --nocapture.
step_proptest_cases() {
  local n
  for n in $IMPLS; do
    echo "== impl-$n"
    w "SME_PROPTEST_CASES=20000 cargo test -q --locked --release -p impl-$n --test spec -- --nocapture differential" 2>&1 | grep -oE 'differential [a-z]+: [0-9]+ cases.*|test result.*'
  done > "$RAW/proptest-cases.txt"
}

step_compile_fail() {
  w "python3 metrics/sme.py compile-fail --run" > "$RAW/compile-fail.txt" 2>&1
  status compile-fail $?
  (cd "$SME/compile-fail/cases" && find . -name '*.rs' | sort) > "$RAW/compile-fail-cases.txt"
}

step_fmt() {
  w "cargo fmt --all --check" > "$RAW/fmt-check.txt" 2>&1
  status fmt-check $?
}

step_loc() {
  local root=$SME
  if ! iso bash -c "cd $SME && cargo fmt --all --check" > /dev/null 2>&1; then
    # Not formatted: count a formatted copy, leaving the slots alone.
    root=$SCRATCH/t/sme-eval-fmt-copy
    rm -rf "$root" && mkdir -p "$root"
    tar -C "$SME" --exclude=./target --exclude=./.git --exclude=./results -cf - . | tar -C "$root" -xf -
    iso bash -c "cd $root && cargo fmt --all" > "$RAW/fmt-copy.txt" 2>&1
    echo "counted in a formatted copy: $root" >> "$RAW/fmt-copy.txt"
  fi
  iso bash -c "cd $root && python3 metrics/sme.py loc --json" > "$RAW/loc.json" 2>&1
  status loc $?
  iso bash -c "cd $root && python3 metrics/sme.py loc" > "$RAW/loc.md" 2>&1
}

step_sizes() {
  w "python3 metrics/sme.py sizes" > "$RAW/sizes.md" 2>&1
  status sizes $?
  w "cargo run -q --locked --release -p bench --bin sizes" > "$RAW/sizes.tsv" 2>&1
}

step_counts() {
  w "cargo run -q --locked --release --manifest-path results/tools/counts/Cargo.toml" > "$RAW/counts.tsv" 2>&1
  status counts $?
}

step_smoke() {
  w "cargo build --locked --release -p bench --bin smoke" > "$RAW/smoke-build.txt" 2>&1
  locked smoke "$RAW/smoke-load.log" iso bash -c "cd $SME && taskset -c 3 cargo run -q --locked --release -p bench --bin smoke" > "$RAW/smoke.txt" 2>&1
  status smoke $?
  # The instruction benches' workload sizes (20k pointer events, 200k cells).
  locked smoke-small "$RAW/smoke-load.log" iso bash -c "cd $SME && SME_SMOKE=1 taskset -c 3 cargo run -q --locked --release -p bench --bin smoke" > "$RAW/smoke-small.txt" 2>&1
  status smoke-small $?
}

step_instructions() {
  local env="PATH=$VALGRIND:\$PATH GUNGRAUN_RUNNER=$RUNNER"
  # Every slot (the measurement), then every crate slot off, so the crate
  # rows are placeholders reporting the harness's fixed overhead (the stub).
  rm -rf "$CARGO_TARGET_DIR/gungraun-all" "$CARGO_TARGET_DIR/gungraun-stub"
  w "$env cargo bench --locked -p bench --features iai --bench instructions --no-run" > "$RAW/instructions-build.txt" 2>&1
  locked instructions "$RAW/instructions-load.log" iso bash -c "cd $SME && $env GUNGRAUN_HOME=$CARGO_TARGET_DIR/gungraun-all cargo bench --locked -p bench --features iai --bench instructions -- --save-summary=pretty-json" > "$RAW/instructions.txt" 2>&1
  status instructions $?
  w "$env cargo bench --locked -p bench --no-default-features --features iai --bench instructions --no-run" > "$RAW/instructions-stub-build.txt" 2>&1
  locked instructions-stub "$RAW/instructions-load.log" iso bash -c "cd $SME && $env GUNGRAUN_HOME=$CARGO_TARGET_DIR/gungraun-stub cargo bench --locked -p bench --no-default-features --features iai --bench instructions -- --save-summary=pretty-json" > "$RAW/instructions-stub.txt" 2>&1
  status instructions-stub $?
  local d
  for d in all stub; do
    consolidate gungraun "$CARGO_TARGET_DIR/gungraun-$d" "$RAW/gungraun-$d.jsonl"
    keep_callgrind "$d" "$CARGO_TARGET_DIR/gungraun-$d"
  done
}

# Build sensitivity: the same benchmark with the three baselines and one crate
# slot only. Build and run both hold the lock (in this evaluation they ran
# between criterion groups).
step_instructions_solo() {
  local env="PATH=$VALGRIND:\$PATH GUNGRAUN_RUNNER=$RUNNER" s
  for s in statig smlang sml finny fsmentry statum state-machines rust-fsm; do
    rm -rf "$CARGO_TARGET_DIR/gungraun-solo-$s"
    locked "instructions solo $s" "$RAW/instructions-load.log" iso bash -c "cd $SME && $env cargo bench --locked -p bench --no-default-features --features iai,$s --bench instructions --no-run > $RAW/instructions-solo-$s-build.txt 2>&1 && $env GUNGRAUN_HOME=$CARGO_TARGET_DIR/gungraun-solo-$s cargo bench --locked -p bench --no-default-features --features iai,$s --bench instructions -- --save-summary=pretty-json" > "$RAW/instructions-solo-$s.txt" 2>&1
    status "instructions-solo-$s" $?
    consolidate gungraun "$CARGO_TARGET_DIR/gungraun-solo-$s" "$RAW/gungraun-solo-$s.jsonl"
  done
}

# Build sensitivity: every slot, and the stub, with the release settings of
# Sekka's Hotaru (apps/hotaru/Cargo.toml: lto = "thin", codegen-units = 1),
# in a target dir of their own. Build and run hold the lock.
step_instructions_lto() {
  local env="PATH=$VALGRIND:\$PATH GUNGRAUN_RUNNER=$RUNNER CARGO_PROFILE_BENCH_LTO=thin CARGO_PROFILE_BENCH_CODEGEN_UNITS=1"
  local t=$SCRATCH/t/sme-eval-lto b feats
  for b in all stub; do
    feats="--features iai"
    [ "$b" = stub ] && feats="--no-default-features --features iai"
    rm -rf "$t/gungraun-lto-$b"
    CARGO_TARGET_DIR=$t locked "instructions lto $b" "$RAW/instructions-load.log" iso bash -c "cd $SME && $env cargo bench --locked -p bench $feats --bench instructions --no-run > $RAW/instructions-lto-$b-build.txt 2>&1 && $env GUNGRAUN_HOME=$t/gungraun-lto-$b cargo bench --locked -p bench $feats --bench instructions -- --save-summary=pretty-json" > "$RAW/instructions-lto-$b.txt" 2>&1
    status "instructions-lto-$b" $?
    consolidate gungraun "$t/gungraun-lto-$b" "$RAW/gungraun-lto-$b.jsonl"
    keep_callgrind "lto-$b" "$t/gungraun-lto-$b"
  done
}

step_compile() {
  local rep profile flag root n
  touch "$SME/metrics/out/compile.jsonl"
  for rep in 1 2 3; do
    for profile in dev release; do
      flag=
      [ "$profile" = release ] && flag=--release
      root=$SCRATCH/t/sme-eval-compile/$profile-$rep
      n=$(wc -l < "$SME/metrics/out/compile.jsonl")
      locked "compile $profile $rep" "$RAW/compile-load.log" iso bash -c "cd $SME && python3 metrics/sme.py compile --root $root $flag" > "$RAW/compile-$profile-$rep.txt" 2>&1
      status "compile-$profile-$rep" $?
      tail -n "+$((n + 1))" "$SME/metrics/out/compile.jsonl" | sed "s/^{/{\"rep\": $rep, /" >> "$RAW/compile.jsonl"
      rm -rf "$root"
    done
  done
}

step_criterion() {
  local bin r g
  w "cargo bench --locked -p bench --bench machines --no-run" > "$RAW/criterion-build.txt" 2>&1
  status criterion-build $?
  bin=$(grep -o '/[^ ()]*/deps/machines-[0-9a-f]*' "$RAW/criterion-build.txt" | tail -1)
  echo "bench binary: $bin" >> "$RAW/criterion-build.txt"
  for r in 1 2; do
    rm -rf "$CARGO_TARGET_DIR/criterion-run$r"
    wait_load "$RAW/criterion-run$r-load.log"
    for g in $BENCH_GROUPS; do
      locked "run $r $g" "$RAW/criterion-run$r-load.log" iso env "CRITERION_HOME=$CARGO_TARGET_DIR/criterion-run$r" taskset -c 3 "$bin" --bench "^$g/" >> "$RAW/criterion-run$r.txt" 2>&1
      status "criterion-run$r-$g" $?
    done
    consolidate criterion "$CARGO_TARGET_DIR/criterion-run$r" "$RAW/criterion-run$r.jsonl"
  done
}

# One more criterion suite (once) with Hotaru's release settings, as for
# instructions-lto; the build also holds the lock.
step_criterion_lto() {
  local t=$SCRATCH/t/sme-eval-lto bin g
  local env="CARGO_PROFILE_BENCH_LTO=thin CARGO_PROFILE_BENCH_CODEGEN_UNITS=1"
  CARGO_TARGET_DIR=$t locked "criterion lto build" "$RAW/criterion-lto-load.log" iso bash -c "cd $SME && $env cargo bench --locked -p bench --bench machines --no-run" > "$RAW/criterion-lto-build.txt" 2>&1
  status criterion-lto-build $?
  bin=$(grep -o '/[^ ()]*/deps/machines-[0-9a-f]*' "$RAW/criterion-lto-build.txt" | tail -1)
  echo "bench binary: $bin" >> "$RAW/criterion-lto-build.txt"
  rm -rf "$t/criterion-lto"
  wait_load "$RAW/criterion-lto-load.log"
  for g in $BENCH_GROUPS; do
    CARGO_TARGET_DIR=$t locked "lto $g" "$RAW/criterion-lto-load.log" iso env "CRITERION_HOME=$t/criterion-lto" taskset -c 3 "$bin" --bench "^$g/" >> "$RAW/criterion-lto.txt" 2>&1
    status "criterion-lto-$g" $?
  done
  consolidate criterion "$t/criterion-lto" "$RAW/criterion-lto.jsonl"
}

# Cross-check: user-space instructions (perf stat) in the timed criterion
# binaries, for the three streams. In criterion's test mode (no --bench) each
# matching benchmark runs once; a filter matching no benchmark in the group
# measures the setup (workload generation), which is subtracted. perf is the
# system's (not in the dev shell); perf_event_paranoid 2 allows counting
# one's own processes.
step_perf_check() {
  local perf=$PERF
  local which bin g i out=$RAW/perf-check.tsv
  echo -e "binary\tgroup\timpl\tinstructions_u" > "$out"
  for which in default lto; do
    if [ "$which" = default ]; then bin=$(grep -o '/[^ ()]*/deps/machines-[0-9a-f]*' "$RAW/criterion-build.txt" | tail -1)
    else bin=$(grep -o '/[^ ()]*/deps/machines-[0-9a-f]*' "$RAW/criterion-lto-build.txt" 2>/dev/null | tail -1); fi
    [ -x "$bin" ] || continue
    for g in pointer-stream wrap-80 wrap-200; do
      for i in none $IMPLS; do
        n=$(flock -o "$LOCK" taskset -c 3 "$perf" stat -x, -e instructions:u "$bin" --test "^$g/$i\$" 2>&1 >/dev/null | grep instructions | cut -d, -f1)
        echo -e "$which\t$g\t$i\t$n" >> "$out"
      done
    done
  done
}

step_tables() {
  # tables.md: every table, from raw/; results.md: those tables with the
  # hand-written notes (results/tools/notes.md) between them.
  iso bash -c "cd $SME && python3 -I results/tools/tables.py" > "$OUT/tables.md" 2> "$RAW/tables-stderr.txt"
  status tables $?
  iso bash -c "cd $SME && python3 -I results/tools/assemble.py" > "$OUT/results.md" 2>> "$RAW/tables-stderr.txt"
  status assemble $?
}

steps=("$@")
[ ${#steps[@]} -eq 0 ] && steps=(env conformance proptest proptest-cases compile-fail fmt loc sizes counts compile smoke instructions instructions-solo instructions-lto criterion criterion-lto perf-check tables)
for s in "${steps[@]}"; do
  echo "$(date -Is) step $s" >> "$RAW/steps.log"
  "step_${s//-/_}"
  echo "$(date -Is) step $s done" >> "$RAW/steps.log"
done
