#!/usr/bin/env bash
# What a window that can't be seen costs: a GL client that draws as fast as
# frame callbacks let it (es2gears_wayland from mesa-demos, swap interval 1),
# on Aizome's headless compositor, visible, then with another Desk shown,
# then minimised. Aizome fires frame callbacks for a surface after a frame
# that showed it, and about once a second for one that doesn't show
# (desktop/aizome/docs/compositor.md).
#
#   tests/performance/design/offscreen.sh OUTDIR AIZOME_BIN GL_CLIENT POWERMETER [REPS] [SECONDS]
#
# For each state: the client's frames (its own fps report), the client's
# and the compositor's CPU time, and the socket's energy (PPT), over
# SECONDS. GLES on the Radeon for both. LOCK wraps the lot.
set -euo pipefail
out=$1 bin=$2 egl=$3 powermeter=$4 reps=${5:-3} secs=${6:-20}
mkdir -p "$out"
tmp=$(mktemp -d "${TMPDIR:-/tmp}"/off.XXXXXX)
export XDG_RUNTIME_DIR=$tmp HOME=$tmp/home XDG_CONFIG_HOME=$tmp/home/.config XDG_STATE_HOME=$tmp/home/.local/state
mkdir -p "$XDG_CONFIG_HOME"
unset WAYLAND_DISPLAY DISPLAY DBUS_SESSION_BUS_ADDRESS AIZOME_SOCKET AIZOME_SHELL_SOCKET
export AIZOME_RENDER_NODE=/dev/dri/renderD128
export __EGL_VENDOR_LIBRARY_FILENAMES=/run/opengl-driver/share/glvnd/egl_vendor.d/50_mesa.json
# Its own process group, so the exit trap stops it and anything it started.
setsid "$bin/aizome" --backend headless --headless-output 1920x1080@1 --renderer gles --no-shell --no-xwayland --no-session \
  --socket wayland-off > "$tmp/aizome.log" 2>&1 &
comp=$!
trap 'kill -- -$comp 2>/dev/null; wait $comp 2>/dev/null' EXIT
export AIZOME_SOCKET=$tmp/aizome.wayland-off.sock
for _ in $(seq 100); do [ -S "$AIZOME_SOCKET" ] && break; sleep 0.1; done
ctl() { "$bin/aizomectl" "$@" || { echo "aizomectl $* failed" >&2; ctl_desks=$("$bin/aizomectl" desks 2>&1); echo "$ctl_desks" >&2; return 1; }; }
cpu() { awk '{print $14 + $15}' "/proc/$1/stat"; }
measure() {
  local state=$1 i=$2 log=$3 client=$4
  local c0 k0 f0
  c0=$(cpu "$client") k0=$(cpu $comp) f0=$(grep -c "frames in" "$log" || true)
  "$powermeter" -i 10 -j "$out/$state-$i.power.json" -- sleep "$secs" 2> /dev/null
  local c1 k1
  c1=$(cpu "$client") k1=$(cpu $comp)
  # es2gears prints "N frames in 5.0 seconds = F FPS" every five seconds.
  local fps
  fps=$(tail -n +$((f0 + 1)) "$log" | grep "frames in" | tail -n 2 | awk '{print $1/$4}' | paste -sd, - || true)
  echo "{\"state\":\"$state\",\"rep\":$i,\"seconds\":$secs,\"client_cpu_ticks\":$((c1 - c0)),\"compositor_cpu_ticks\":$((k1 - k0)),\"clk_tck\":$(getconf CLK_TCK),\"fps_reports\":[${fps}],\"load\":\"$(cut -d' ' -f1-3 /proc/loadavg)\"}" >> "$out/runs.jsonl"
}
"$powermeter" -i 10 -j "$out/no-client.power.json" -- sleep "$secs" 2> /dev/null
for i in $(seq 1 "$reps"); do
  log=$tmp/egl-$i.log
  # Line-buffered, so its reports reach the log as it prints them.
  WAYLAND_DISPLAY=wayland-off stdbuf -oL "$egl" > "$log" 2>&1 &
  client=$!
  sleep 3
  id=$(ctl --json windows | sed -n 's/.*"id": *\([0-9]*\).*/\1/p' | head -1)
  [ -n "$id" ] || { echo "no window from $egl:"; cat "$log"; exit 1; }
  ctl focus "$id" > /dev/null
  sleep 2
  measure visible "$i" "$log" $client
  # Desk 2 (a session starts with more than one), as frame_bench does.
  ctl set-desk 2 > /dev/null
  sleep 3
  measure other-desk "$i" "$log" $client
  ctl set-desk 1 > /dev/null
  sleep 2
  ctl minimise "$id" > /dev/null
  sleep 3
  measure minimised "$i" "$log" $client
  kill $client
  wait $client 2> /dev/null || true
  sleep 2
done
"$powermeter" -i 10 -j "$out/no-client-after.power.json" -- sleep "$secs" 2> /dev/null
