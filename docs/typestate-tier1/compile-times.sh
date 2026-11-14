#!/usr/bin/env bash
# As run for docs/typestate-tier1.md. $RUN was the session's isolation wrapper, equivalent to
# docs/state-machine-crates/eval/isolate from #282: an empty environment with the named dev shell,
# a private HOME and runtime dir, and CARGO_TARGET_DIR from REPRO_TARGET. NM points the linker at
# NetworkManager's libnm, which Genkan's packages link. Run it as
#   flock -o ~/.cache/hotaru-runnymede-benchmark.lock ./compile-times.sh MAIN_WORKTREE BRANCH_WORKTREE 5
# Compile-time impact of the tier 1 typestate work: main against the branch,
# interleaved per repetition so host load falls on both alike.
#
#   compile-times.sh BASE_WORKTREE BRANCH_WORKTREE [REPS]
#
# For each package below, in each worktree: warm its dependencies, then REPS
# times, alternating base and branch:
#   crate: `cargo clean -p PKG`, then `cargo build -p PKG` (the package alone,
#          its dependencies already built), dev profile;
#   incr:  touch the package's src/lib.rs or src/main.rs, then `cargo build -p
#          PKG` with CARGO_INCREMENTAL=1.
# Prints "LABEL PKG KIND REP SECONDS LOAD1" lines.
set -u
base=$1 branch=$2 reps=${3:-5}
RUN=/tmp/claude-1000/tk/i/bin/run
# shell|workspace|package|touched file (relative to the workspace)
PKGS=(
  "aizome|desktop/aizome|aizome-typestate|crates/aizome-typestate/src/lib.rs"
  "aizome|desktop/aizome|aizome-sys|crates/aizome-sys/src/lib.rs"
  "aizome|desktop/aizome|aizome-tansu|apps/tansu/src/lib.rs"
  "aizome|desktop/aizome|aizome-hako|apps/hako/src/lib.rs"
  "aizome|desktop/aizome|aizome-yukimi|apps/yukimi/src/lib.rs"
  "kakehashi|apps/kakehashi|kakehashi-core|core/src/lib.rs"
  "kakehashi|apps/kakehashi|kakehashi|app/src/main.rs"
  "kakehashi|apps/kakehashi|kakehashi-helper|helper/src/main.rs"
  "kakehashi|apps/kakehashi|kakehashi-write|write/src/lib.rs"
  "default|installer/genkan/rust|calamares-nixos|src/lib.rs"
  "aizome|installer/genkan/ui|genkan|src/lib.rs"
  "default|apps/yukimi|yukimi-system|crates/yukimi-system/src/lib.rs"
  "default|apps/yukimi|yukimi-helper|yukimi-helper/src/main.rs"
)
NM=/nix/store/yl9lagnfzmlqmn1v3vl839q84c320nak-networkmanager-1.58.1/lib
one() { # label worktree shell ws pkg file kind rep
  local label=$1 wt=$2 shell=$3 ws=$4 pkg=$5 file=$6 kind=$7 rep=$8
  REPRO_TARGET=/tmp/claude-1000/tk/i/t/ct-$label-$(echo "$ws" | tr / -) CARGO_BUILD_JOBS=8 taskset -c 2-7,10-15 \
    "$RUN" "$shell" "ct-$label" bash -c '
    cd "$1/$2" || exit 1
    export LIBRARY_PATH='"$NM"' LD_LIBRARY_PATH='"$NM"'
    pkg=$3 file=$4 kind=$5
    case $kind in
      warm) cargo build -q -p "$pkg" >/dev/null 2>&1 && CARGO_INCREMENTAL=1 cargo build -q -p "$pkg" >/dev/null 2>&1; echo $?; exit 0 ;;
      crate) cargo clean -q -p "$pkg"; s=$(date +%s.%N); cargo build -q -p "$pkg" >/dev/null 2>&1 || exit 3; e=$(date +%s.%N) ;;
      incr) touch "$file"; s=$(date +%s.%N); CARGO_INCREMENTAL=1 cargo build -q -p "$pkg" >/dev/null 2>&1 || exit 3; e=$(date +%s.%N) ;;
    esac
    awk "BEGIN{printf \"%.3f\n\", $e-$s}"
  ' _ "$wt" "$ws" "$pkg" "$file" "$kind"
}
for spec in "${PKGS[@]}"; do
  IFS='|' read -r shell ws pkg file <<<"$spec"
  for side in base branch; do
    wt=$base; [ $side = branch ] && wt=$branch
    if [ ! -f "$wt/$ws/$file" ]; then echo "$side $pkg absent"; continue; fi
    w=$(one "$side" "$wt" "$shell" "$ws" "$pkg" "$file" warm 0)
    [ "$w" = 0 ] || echo "$side $pkg warm-failed"
  done
  for kind in crate incr; do
    for i in $(seq "$reps"); do
      for side in base branch; do
        wt=$base; [ $side = branch ] && wt=$branch
        [ -f "$wt/$ws/$file" ] || continue
        t=$(one "$side" "$wt" "$shell" "$ws" "$pkg" "$file" "$kind" "$i")
        echo "$side $pkg $kind $i $t $(cut -d' ' -f1 /proc/loadavg)"
      done
    done
  done
done
