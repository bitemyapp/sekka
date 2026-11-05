#!/usr/bin/env bash
# Run one command inside a private headless sway (the environment probe.py
# and design/window-cost.py give programs), then tear it all down.
#
#   with-sway.sh CMD [ARGS...]
#
# Sway and the D-Bus daemon run in process groups of their own and are
# killed on exit, with anything else left holding the private runtime dir.
# Needs sway, swaymsg and dbus-daemon on PATH.
set -euo pipefail
tmp=$(mktemp -d /tmp/claude-1000/ws.XXXXXX)
cleanup() {
  [ -n "${bus:-}" ] && kill -TERM -- "-$bus" 2>/dev/null || true
  [ -n "${sway:-}" ] && kill -TERM -- "-$sway" 2>/dev/null || true
  sleep 0.3
  for p in /proc/[0-9]*; do
    if { tr '\0' '\n' <"$p/environ"; } 2>/dev/null | grep -qx "XDG_RUNTIME_DIR=$tmp"; then
      kill -KILL "${p#/proc/}" 2>/dev/null || true
    fi
  done
  rm -rf "$tmp"
}
trap cleanup EXIT
unset WAYLAND_DISPLAY DISPLAY WAYLAND_SOCKET
export XDG_RUNTIME_DIR=$tmp HOME=$tmp/home XDG_CONFIG_HOME=$tmp/home/.config \
  XDG_CACHE_HOME=$tmp/home/.cache XDG_DATA_HOME=$tmp/home/.local/share XDG_STATE_HOME=$tmp/home/.local/state \
  WLR_BACKENDS=headless WLR_RENDERER=gles2 WLR_LIBINPUT_NO_DEVICES=1 \
  WLR_RENDER_DRM_DEVICE=${WLR_RENDER_DRM_DEVICE:-/dev/dri/renderD128} \
  __EGL_VENDOR_LIBRARY_FILENAMES=${__EGL_VENDOR_LIBRARY_FILENAMES:-/run/opengl-driver/share/glvnd/egl_vendor.d/50_mesa.json} \
  GDK_BACKEND=wayland QT_QPA_PLATFORM=wayland XDG_SESSION_TYPE=wayland
mkdir -p "$XDG_CONFIG_HOME"
cat >"$tmp/sway.conf" <<'EOF'
output HEADLESS-1 resolution 1920x1080 scale 1
xwayland disable
default_border none
focus_on_window_activation focus
for_window [title=".*"] floating enable
EOF
setsid sway -c "$tmp/sway.conf" >/dev/null 2>"$tmp/sway.log" &
sway=$!
for _ in $(seq 100); do
  ls "$tmp"/wayland-? >/dev/null 2>&1 && ls "$tmp"/sway-ipc.* >/dev/null 2>&1 && break
  sleep 0.1
done
WAYLAND_DISPLAY=$(basename "$(ls "$tmp"/wayland-? | head -1)")
SWAYSOCK=$(ls "$tmp"/sway-ipc.* | head -1)
export WAYLAND_DISPLAY SWAYSOCK
setsid dbus-daemon --session --nofork --nopidfile --address="unix:path=$tmp/bus" >/dev/null 2>&1 &
bus=$!
export DBUS_SESSION_BUS_ADDRESS="unix:path=$tmp/bus"
sleep 0.5
"$@"
