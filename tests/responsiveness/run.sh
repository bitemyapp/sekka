#!/usr/bin/env bash
# How responsive the desktop stays while builds run. See
# docs/responsiveness.md.
#
#   run.sh measure LABEL     start a measurement as a user service and return
#   run.sh summary LABEL     print the measurement's summary as JSON
#   run.sh compare A B       print a Markdown comparison of two measurements
#
# A measurement runs on the live session for about 15 minutes, with idle,
# sleep and lid-switch inhibited. Results go to .work/responsiveness/LABEL.
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
state=${RESPONSIVENESS_STATE:-$root/.work/responsiveness}
here=$root/tests/responsiveness
system_path=/run/wrappers/bin:/etc/profiles/per-user/$USER/bin:/run/current-system/sw/bin

tools() {
  local paths
  paths=$(nix build --no-update-lock-file --inputs-from "$root" nixpkgs#fio nixpkgs#jq \
    --no-link --print-out-paths)
  fio=$(grep -- '-fio-' <<<"$paths")/bin/fio
  jq=$(grep -- '-jq-.*-bin$' <<<"$paths")/bin/jq
}

log() { printf '%s %s\n' "$(date +%T)" "$*" | tee -a "$out/log"; }

# Prepared once and reused, so every measurement uses the same probe binary,
# frame workload, build environment and test files.
setup() {
  mkdir -p "$state/bin" "$state/data"
  probe=$state/bin/probe
  [ -x "$probe" ] || rustc -O --edition 2021 "$here/probe.rs" -o "$probe"
  # Frame work that takes 2 ms on an idle CPU, calibrated by the first run.
  [ -s "$state/work" ] || "$probe" calibrate --work-us 2000 >"$state/work"
  work=$(cat "$state/work")
  [ -s "$state/devenv.sh" ] || nix print-dev-env --no-update-lock-file "$root#default" >"$state/devenv.sh"
  [ -s "$state/data/read.dat" ] ||
    "$fio" --name=layout --filename="$state/data/read.dat" --size=1G --rw=write --bs=1M \
      --direct=1 --output=/dev/null
  [ -s "$state/data/fsync.dat" ] ||
    "$fio" --name=layout --filename="$state/data/fsync.dat" --size=64M --rw=write --bs=1M \
      --output=/dev/null
}

meta() {
  {
    echo "label: $label"
    echo "date: $(date -Is)"
    echo "revision: $(git -C "$root" rev-parse HEAD)"
    echo "kernel: $(uname -r)"
    echo "cmdline: $(cat /proc/cmdline)"
    for d in /sys/block/nvme*n1; do echo "scheduler $(basename "$d"): $(cat "$d/queue/scheduler")"; done
    echo "ac online: $(cat /sys/class/power_supply/AC*/online 2>/dev/null)"
    echo "platform profile: $(cat /sys/firmware/acpi/platform_profile 2>/dev/null)"
    echo "energy preference: $(cat /sys/devices/system/cpu/cpu0/cpufreq/energy_performance_preference 2>/dev/null)"
    echo "uptime s: $(cut -d' ' -f1 /proc/uptime)"
    echo "frame work iterations: $work"
    # A new shell, once ananicy has seen it.
    echo "shell nice: $(bash -c 'sleep 2; cut -d" " -f19 /proc/$$/stat')"
    echo "nix-daemon class, nice: $(ps -o cls=,ni= -C nix-daemon | sort -u | tr '\n' ' ')"
    free -m
  } >"$out/meta.txt"
}

unit() {
  local name=$1
  shift
  systemd-run --user --quiet --collect --unit="sekka-resp-$name" --slice=app-graphical.slice \
    --property=Nice=0 --setenv=PATH="$PATH" -- "$@"
}

probes=(wake frame frame-short exec read fsync)

start_probes() {
  local dir=$1
  unit wake "$probe" wake --period-us 1000 --stop "$dir/stop" --out "$dir/wake.json"
  unit frame "$probe" frame --period-us 8333 --work "$work" --stop "$dir/stop" --out "$dir/frame.json"
  # The same frames with a 1 ms time slice instead of the default 2.8 ms.
  unit frame-short /run/current-system/sw/bin/chrt -o -T 1000000 0 "$probe" frame --period-us 8333 --work "$work" \
    --stop "$dir/stop" --out "$dir/frame-short.json"
  unit exec "$probe" exec --interval-ms 250 --stop "$dir/stop" --out "$dir/exec.json" \
    -- /run/current-system/sw/bin/foot --version
  unit read "$fio" --name=read --filename="$state/data/read.dat" --size=1G --rw=randread --bs=4k \
    --direct=1 --ioengine=psync --rate_iops=200 --time_based --runtime=3600 \
    --output-format=json --output="$dir/read.json"
  unit fsync "$fio" --name=fsync --filename="$state/data/fsync.dat" --size=64M --rw=write --bs=4k \
    --fsync=1 --rate_iops=20 --time_based --runtime=3600 \
    --output-format=json --output="$dir/fsync.json"
}

stop_probes() {
  local dir=$1 p
  touch "$dir/stop"
  systemctl --user kill --signal=SIGINT sekka-resp-read.service sekka-resp-fsync.service 2>/dev/null || true
  for p in "${probes[@]}"; do
    while systemctl --user is-active --quiet "sekka-resp-$p.service"; do sleep 0.5; done
  done
}

psi() {
  local r
  for r in cpu io memory; do
    awk -v r="$r" '{ split($5, t, "="); print r "_" $1, t[2] }' "/proc/pressure/$r"
  done
}

# Scheduling class and nice value of build processes, every 5 s.
priorities() {
  local dir=$1
  while [ ! -e "$dir/stop" ]; do
    ps -eo cls=,ni=,comm=,cgroup= |
      awk '$4 ~ /sekka-resp-terminal|nix-daemon/ && $3 ~ /^(rustc|cc1|cc1plus|cargo|cc|ld|bash|foot|build-script-b|make)$/ {
             where = ($4 ~ /nix-daemon/) ? "nix" : "terminal"; print where, $3, $1, $2 }' |
      sort | uniq -c >>"$dir/priorities.log"
    echo --- >>"$dir/priorities.log"
    sleep 5
  done
}

nix_load() {
  nix build --no-update-lock-file --impure --no-link --quiet --expr "
    let
      pkgs = (builtins.getFlake \"git+file://$root\").inputs.nixpkgs.legacyPackages.x86_64-linux;
      fresh = p: p.overrideAttrs { SEKKA_RESPONSIVENESS = \"$1\"; doCheck = false; };
    in pkgs.linkFarmFromDrvs \"responsiveness-load\" (map fresh [ pkgs.ripgrep pkgs.fd pkgs.bat ])"
}

terminal_load() {
  local dir=$1
  rm -rf "$state/target"
  systemd-run --user --quiet --collect --wait --unit=sekka-resp-terminal --slice=app-graphical.slice \
    --property=Nice=0 --setenv=PATH="$PATH" -- \
    /run/current-system/sw/bin/bash "$here/terminal.sh" "$state" "$dir" "$probe" "$work" "$root"
}

# measure NAME LOAD...: the probes run from just before the load starts until it ends.
measure() {
  local name=$1 dir=$out/$1
  shift
  mkdir -p "$dir"
  rm -f "$dir/stop" "$dir/terminal.stop"
  log "start $name"
  psi >"$dir/psi.before"
  start_probes "$dir"
  priorities "$dir" &
  local sampler=$! t0 t1
  sleep 2
  t0=$(date +%s.%N)
  "$@" || echo failed >"$dir/load.failed"
  t1=$(date +%s.%N)
  stop_probes "$dir"
  wait "$sampler"
  psi >"$dir/psi.after"
  echo "$t0 $t1" >"$dir/load.times"
  log "end $name: $(awk '{ printf "%.1f s", $2 - $1 }' "$dir/load.times")$([ -e "$dir/load.failed" ] && echo ', load FAILED')"
}

cooldown() { sleep 30; }

run_measurement() {
  out=$state/$label
  mkdir -p "$out"
  tools
  setup
  meta
  log "warm-up: Nix build and terminal build, unmeasured"
  mkdir -p "$out/warm-up"
  nix_load "$label-warm-up-$(date +%s%N)" || log "warm-up Nix build failed"
  terminal_load "$out/warm-up" || log "warm-up terminal build failed"
  cooldown
  measure idle sleep 60
  local i
  for i in 1 2; do
    cooldown
    measure "nix-$i" nix_load "$label-nix-$i-$(date +%s%N)"
    cooldown
    measure "terminal-$i" terminal_load "$out/terminal-$i"
  done
  summary >"$out/summary.json"
  log "done: $out/summary.json"
}

# Seconds of stall over a scenario, from PSI totals in microseconds.
summary() {
  [ -n "${jq:-}" ] || tools
  local dir name
  for dir in "$out"/*/; do
    name=$(basename "$dir")
    [ -e "$dir/load.times" ] || continue
    "$jq" -n \
      --arg name "$name" \
      --arg times "$(cat "$dir/load.times")" \
      --arg failed "$([ -e "$dir/load.failed" ] && echo true || echo false)" \
      --rawfile before "$dir/psi.before" --rawfile after "$dir/psi.after" \
      --slurpfile wake "$dir/wake.json" \
      --slurpfile frame "$dir/frame.json" \
      --slurpfile short "$dir/frame-short.json" \
      --slurpfile exec "$dir/exec.json" \
      --slurpfile read <(sed -n '/^{/,$p' "$dir/read.json") \
      --slurpfile fsync <(sed -n '/^{/,$p' "$dir/fsync.json") \
      --slurpfile terminal <(cat "$dir/terminal-frame.json" 2>/dev/null || true) '
      def totals($s): $s | split("\n") | map(select(length > 0) | split(" ") | {(.[0]): (.[1] | tonumber)}) | add;
      def us(p): {p50_us: (p.percentile["50.000000"] / 1000), p99_us: (p.percentile["99.000000"] / 1000),
                  p999_us: (p.percentile["99.900000"] / 1000), max_us: (p.max / 1000)};
      ($times | split(" ") | map(tonumber)) as $t
      | totals($before) as $b | totals($after) as $a
      | {($name): {
          load_s: ($t[1] - $t[0]),
          load_failed: ($failed == "true"),
          stall_s: ($a | with_entries(.value = ((.value - $b[.key]) / 1e6))),
          wake: $wake[0].wake_late,
          frame: ($frame[0] | {frames, missed, missed_twice, wake_late, frame_time}),
          frame_short_slice: ($short[0] | {frames, missed, missed_twice, wake_late, frame_time}),
          terminal_frame: (if $terminal | length > 0 then $terminal[0] | {frames, missed, missed_twice, wake_late, frame_time} else null end),
          exec: $exec[0].exec,
          read_4k: us($read[0].jobs[0].read.clat_ns),
          fsync: us($fsync[0].jobs[0].sync.lat_ns)
        }}'
  done | "$jq" -s --arg label "$label" --rawfile meta "$out/meta.txt" '{label: $label, meta: $meta, scenarios: add}'
}

compare() {
  tools
  "$jq" -r -n --slurpfile a "$state/$1/summary.json" --slurpfile b "$state/$2/summary.json" '
    def avg(f): [.[] | f | select(. != null)] | if length == 0 then null else add / length end;
    def runs($s; $prefix): [$s.scenarios | to_entries[] | select(.key | startswith($prefix)) | .value];
    def fmt: if . == null then "–" elif . >= 100 then (round | tostring) else (. * 10 | round / 10 | tostring) end;
    # Scenario runs are averaged: idle, then nix-1 and nix-2, then terminal-1 and terminal-2.
    def row($label; f):
      [["idle", "nix", "terminal"][] as $p | ($a[0], $b[0]) as $s | runs($s; $p) | avg(f) | fmt]
      | "| \($label) | " + join(" | ") + " |";
    ($a[0].label) as $x | ($b[0].label) as $y
    | "| Metric | idle, \($x) | idle, \($y) | Nix build, \($x) | Nix build, \($y) | terminal build, \($x) | terminal build, \($y) |",
    "|---|---|---|---|---|---|---|",
    row("Load duration (s)"; .load_s),
    row("CPU stall, some (s)"; .stall_s.cpu_some),
    row("I/O stall, some (s)"; .stall_s.io_some),
    row("I/O stall, full (s)"; .stall_s.io_full),
    row("Memory stall, full (s)"; .stall_s.memory_full),
    row("Wakeup late p99 (µs)"; .wake.p99_us),
    row("Wakeup late max (µs)"; .wake.max_us),
    row("Frame time p99 (µs)"; .frame.frame_time.p99_us),
    row("Frames missed at 120 Hz (%)"; (.frame.missed / .frame.frames * 100)),
    row("Frames over 16.7 ms (%)"; (.frame.missed_twice / .frame.frames * 100)),
    row("1 ms slice: frame time p99 (µs)"; .frame_short_slice.frame_time.p99_us),
    row("1 ms slice: frames missed (%)"; (.frame_short_slice.missed / .frame_short_slice.frames * 100)),
    row("Terminal frames missed (%)"; (if .terminal_frame then .terminal_frame.missed / .terminal_frame.frames * 100 else null end)),
    row("Terminal frame time p99 (µs)"; .terminal_frame.frame_time.p99_us),
    row("Program start p50 (µs)"; .exec.p50_us),
    row("Program start p99 (µs)"; .exec.p99_us),
    row("4K read p50 (µs)"; .read_4k.p50_us),
    row("4K read p99 (µs)"; .read_4k.p99_us),
    row("4K read p99.9 (µs)"; .read_4k.p999_us),
    row("fsync p50 (µs)"; .fsync.p50_us),
    row("fsync p99 (µs)"; .fsync.p99_us)'
}

case ${1:-} in
# A short check of the probes and the summary: 10 s idle, no builds.
smoke)
  label=${2:?usage: run.sh smoke LABEL}
  out=$state/$label
  mkdir -p "$out"
  tools
  setup
  meta
  measure idle sleep 10
  summary >"$out/summary.json"
  ;;
measure)
  label=${2:?usage: run.sh measure LABEL}
  if [ -z "${RESPONSIVENESS_INSIDE:-}" ]; then
    systemd-run --user --quiet --collect --unit=sekka-responsiveness --slice=app.slice \
      --setenv=RESPONSIVENESS_INSIDE=1 --setenv=PATH="$system_path" --setenv=RESPONSIVENESS_STATE="$state" -- \
      systemd-inhibit --what=idle:sleep:handle-lid-switch --who="Sekka responsiveness" \
      --why="Measuring responsiveness under load" --mode=block \
      "$here/run.sh" measure "$label"
    echo "Measuring as sekka-responsiveness.service; log: $state/$label/log"
    exit
  fi
  run_measurement
  ;;
summary)
  label=${2:?usage: run.sh summary LABEL}
  out=$state/$label
  summary
  ;;
compare) compare "${2:?usage: run.sh compare A B}" "${3:?usage: run.sh compare A B}" ;;
*)
  echo "usage: run.sh measure LABEL | summary LABEL | compare A B" >&2
  exit 2
  ;;
esac
