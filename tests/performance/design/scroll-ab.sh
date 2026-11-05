#!/usr/bin/env bash
# What moving pixels saves when a view scrolls: the kit's gallery scrolling
# its 10,000-row list and table two rows every 20 ms (GALLERY_DEMO=
# bench-scroll), and its editor a line every 20 ms (bench-scroll-editor),
# with the kit moving what was drawn (the default) and drawing it again
# (AIZOME_NO_PIXEL_SCROLL=1), in a private headless sway at 2880 × 1800,
# scale 2, on the Radeon.
#
#   tests/performance/design/scroll-ab.sh OUTDIR GALLERY POWERMETER [REPS] [SECONDS]
#
# Each run records Sidestep's frame statistics (SIDESTEP_FRAME_STATS=1: per
# frame record, rasterise, present and the share of the window redrawn,
# summarised every five seconds), the gallery's CPU time over the run
# (utime+stime) and the socket's energy (powermeter, PPT). Runs alternate
# (pixels moved, drawn again, ...). Needs sway in PATH; a cargo-built
# gallery also needs Aizome's development shell for its library path. LOCK
# wraps the lot.
set -euo pipefail
out=$1 gallery=$2 powermeter=$3 reps=${4:-3} secs=${5:-30}
mkdir -p "$out"
tmp=$(mktemp -d "${TMPDIR:-/tmp}"/sab.XXXXXX)
export XDG_RUNTIME_DIR=$tmp HOME=$tmp/home XDG_CONFIG_HOME=$tmp/home/.config XDG_CACHE_HOME=$tmp/home/.cache
mkdir -p "$XDG_CONFIG_HOME"
unset WAYLAND_DISPLAY DISPLAY DBUS_SESSION_BUS_ADDRESS AIZOME_SOCKET AIZOME_SHELL_SOCKET
export WLR_BACKENDS=headless WLR_RENDERER=gles2 WLR_LIBINPUT_NO_DEVICES=1 WLR_RENDER_DRM_DEVICE=/dev/dri/renderD128
export __EGL_VENDOR_LIBRARY_FILENAMES=/run/opengl-driver/share/glvnd/egl_vendor.d/50_mesa.json
printf 'output HEADLESS-1 resolution 2880x1800 scale 2\nxwayland disable\ndefault_border none\n' > "$tmp/sway.conf"
# Its own process group, so the exit trap stops sway and everything it
# started (with no session bus, the sway wrapper runs it under
# dbus-run-session), and nothing outlives the run holding inherited files.
setsid sway -c "$tmp/sway.conf" > "$tmp/sway.log" 2>&1 &
sway_pid=$!
trap 'kill -- -$sway_pid 2>/dev/null; wait $sway_pid 2>/dev/null' EXIT
for _ in $(seq 100); do
  sock=$(cd "$tmp" && ls wayland-? 2>/dev/null | head -1) && [ -n "$sock" ] && break
  sleep 0.1
done
export WAYLAND_DISPLAY=$sock
cpu() { awk '{print $14 + $15}' "/proc/$1/stat"; }
run() {
  local demo=$1 mode=$2 i=$3 name="$1-$2-$3"
  local extra=()
  [ "$mode" = redraw ] && extra=(AIZOME_NO_PIXEL_SCROLL=1)
  env "${extra[@]}" GALLERY_DEMO="$demo" SIDESTEP_FRAME_STATS=1 "$gallery" 2> "$out/$name.stats" > /dev/null &
  local pid=$!
  sleep 6 # opened, first frames drawn, the demo started (1.2 s) and settled
  local c0
  c0=$(cpu $pid)
  "$powermeter" -i 10 -j "$out/$name.power.json" -- sleep "$secs" 2> /dev/null
  local c1
  c1=$(cpu $pid)
  kill $pid
  wait $pid 2> /dev/null || true
  echo "{\"demo\":\"$demo\",\"mode\":\"$mode\",\"rep\":$i,\"seconds\":$secs,\"cpu_ticks\":$((c1 - c0)),\"clk_tck\":$(getconf CLK_TCK),\"load\":\"$(cut -d' ' -f1-3 /proc/loadavg)\"}" >> "$out/runs.jsonl"
  sleep 3
}
for i in $(seq 1 "$reps"); do
  for demo in bench-scroll bench-scroll-editor; do
    run "$demo" moved "$i"
    run "$demo" redraw "$i"
  done
done
