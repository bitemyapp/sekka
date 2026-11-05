#!/usr/bin/env bash
# Which threads of an app wake while it rests: each command (a shell
# command line) runs in a private headless sway on the Radeon with a private
# D-Bus session; after 6 s, every thread's context switches (voluntary and
# not, its children's threads too) are counted over 10 s.
#
#   nix shell --inputs-from . nixpkgs#sway nixpkgs#dbus -c \
#     tests/performance/design/thread-wakeups.sh "aizome-hako DIR" "aizome-shiori FILE.pdf" ...
set -euo pipefail
tmp=$(mktemp -d "${TMPDIR:-/tmp}"/tw.XXXXXX)
export XDG_RUNTIME_DIR=$tmp HOME=$tmp/home XDG_CONFIG_HOME=$tmp/home/.config XDG_CACHE_HOME=$tmp/home/.cache XDG_DATA_HOME=$tmp/home/.local/share XDG_STATE_HOME=$tmp/home/.local/state
mkdir -p $XDG_CONFIG_HOME
unset WAYLAND_DISPLAY DISPLAY AIZOME_SOCKET AIZOME_SHELL_SOCKET
export WLR_BACKENDS=headless WLR_RENDERER=gles2 WLR_LIBINPUT_NO_DEVICES=1 WLR_RENDER_DRM_DEVICE=/dev/dri/renderD128 __EGL_VENDOR_LIBRARY_FILENAMES=/run/opengl-driver/share/glvnd/egl_vendor.d/50_mesa.json
printf 'output HEADLESS-1 resolution 1920x1080\nxwayland disable\n' > $tmp/sway.conf
sway -c $tmp/sway.conf >/dev/null 2>&1 & sp=$!
sleep 1; export WAYLAND_DISPLAY=$(cd $tmp && ls wayland-? | head -1)
eval $(dbus-launch --sh-syntax)
for app in "$@"; do
  sh -c "$app" >/dev/null 2>&1 & ap=$!
  sleep 6
  snap() { for p in $(cat /proc/$ap/task/*/children 2>/dev/null) $ap; do for t in /proc/$p/task/*; do echo "$p:$(cat $t/comm) $(awk '/ctxt_switches/{s+=$2} END{print s}' $t/status)"; done; done; }
  snap > $tmp/a; sleep 10; snap > $tmp/b
  echo "== $app"; join <(sort $tmp/a) <(sort $tmp/b) | awk '{d=$3-$2; if (d>0) printf "  %-40s %5.1f/s\n", $1, d/10}' | sort -k2 -rn | head -8
  kill $ap; wait $ap 2>/dev/null || true; sleep 1
done
kill $DBUS_SESSION_BUS_PID $sp
