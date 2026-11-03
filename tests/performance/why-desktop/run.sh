#!/usr/bin/env bash
# docs/performance/why-desktop.md: the analyses of the desktop comparison's
# trial data, and the headless host runs.
#
#   tests/performance/why-desktop/run.sh pins PINS        # build and pin what the host runs use
#   tests/performance/why-desktop/run.sh analyse          # the VM data → data/why-desktop/*.json
#   tests/performance/why-desktop/run.sh headless PINS OUT [ROUNDS]
#
# LOCK (optional) is a command each headless run is run under, and
# LOCK_BUILD one for the builds; here they were tests/performance/lock-measure
# and lock-build. The headless runs use two CPUs (WHY_CPUS, default 2,3).
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../../.." && pwd)
cmd=${1:?usage: run.sh pins|analyse|headless ...}
shift

case $cmd in
pins)
  pins=${1:?PINS directory}
  mkdir -p "$pins"
  b() { ${LOCK_BUILD:-} nix build --impure -f "$repo/tests/performance/desktops" --out-link "$pins/$1" "$2"; }
  # The desktops from the VMs' Nixpkgs, each VM's system-path, tools.
  b gnome-shell pkgs.gnome-shell
  b mutter pkgs.mutter
  b kwin pkgs.kdePackages.kwin
  b plasma-workspace pkgs.kdePackages.plasma-workspace
  b gnome-sw gnome.config.system.path
  b plasma-sw plasma.config.system.path
  b aizome-sw aizome.config.system.path
  # Aizome exactly as in the VMs (versions.json); the others come from this
  # checkout's Nixpkgs, which may have moved on since the VMs were built.
  ${LOCK_BUILD:-} nix build --out-link "$pins/aizome" \
    "$(grep -o '"aizome":"[^"]*"' "$repo/docs/performance/data/desktops/versions.json" | cut -d'"' -f4)"
  b perf pkgs.perf
  b python3 pkgs.python3
  b dbus pkgs.dbus
  ${LOCK_BUILD:-} nix build --impure -f "$here" --out-link "$pins/probe" probe
  # Sources read for the write-up.
  b mutter-src pkgs.mutter.src
  b gnome-shell-src pkgs.gnome-shell.src
  b kwin-src pkgs.kdePackages.kwin.src
  b plasma-workspace-src pkgs.kdePackages.plasma-workspace.src
  ;;
analyse)
  py=$(nix build --impure -f "$repo/tests/performance/desktops" --no-link --print-out-paths host)/bin/python3
  trials=$repo/docs/performance/data/desktops/trials
  out=$repo/docs/performance/data/why-desktop
  mkdir -p "$out"
  "$py" -I "$here/latency-phase.py" "$trials" "$out/latency-phase.json"
  "$py" -I "$here/latency-streaks.py" "$trials" "$out/latency-streaks.json"
  "$py" -I "$here/memory-split.py" "$trials" "$out/memory-split.json"
  "$py" -I "$here/memory-gap.py" "$trials" "$out/memory-gap.json"
  "$py" -I "$here/session-cpu.py" "$trials" "$out/session-cpu.json"
  for d in aizome gnome plasma; do
    for t in r1 r2 r3; do
      "$py" -I "$here/login-timeline.py" "$trials/$d-4096-$t" 6 \
        | grep -E 'SCREEN|session opened for user demo|Started|Starting|Reached target|Finished Splash|Consumed|GNOME Shell started|Registering|Shutting down GNOME|Running GNOME Shell|Using Wayland display|aizome_shell::shell|polkit agent registered|Started Aizome' \
        | grep -v 'Ignoring duplicate\|not named after' | cut -c1-170 > "$out/login-timeline-$d-$t.txt"
    done
  done
  ;;
headless)
  pins=${1:?PINS}
  out=${2:?OUT}
  rounds=${3:-2}
  py=$pins/python3/bin/python3
  mkdir -p "$out"
  one() { ${LOCK:-} "$py" -I "$here/headless.py" "$@" | tee -a "$out/runs.log"; }
  # Clean runs (no profiler), the desktops alternating.
  for r in $(seq 1 "$rounds"); do
    for d in gnome plasma aizome; do one "$d" "$out" "$pins" --tag "r$r"; done
  done
  # Each desktop with a plain background instead of Sekka's wallpaper
  # (GNOME Shell never finishes starting without a picture, and the
  # summary leaves that run out), and GNOME with the same picture cropped
  # to the screen's size.
  for d in gnome plasma aizome; do one "$d" "$out" "$pins" --tag r1 --wallpaper off; done
  small=$out/foggy-forest-1920x1080.jpg
  ${LOCK_BUILD:-} nix shell --inputs-from "$repo" nixpkgs#imagemagick -c magick \
    "$(readlink -f "$pins/gnome-sw/share/backgrounds/tatami/foggy-forest.jpg")" \
    -resize 1920x1080^ -gravity center -extent 1920x1080 -quality 92 "$small"
  for r in 1 2; do one gnome "$out" "$pins" --tag "r$r" --wallpaper "$small"; done
  # Profiled runs, for attribution only.
  for d in gnome plasma aizome; do
    one "$d" "$out" "$pins" --tag r1 --perf
    "$here/perf-summary.sh" "$out/$d-r1-perf" "$pins/perf/bin/perf" > "$out/$d-r1-perf/perf-summary.txt"
  done
  "$py" -I "$here/headless-summary.py" "$out" "$out/summary.json" > "$out/summary.md"
  ;;
*)
  echo "unknown command $cmd" >&2
  exit 2
  ;;
esac
