#!/usr/bin/env bash
# Render-node-only integration: never acquires KMS or replaces the desktop.
# Requires built .#aizome / .#gamescope, vulkan-tools and xwayland-satellite.
set -euo pipefail
: "${AIZOME_RENDER_NODE:?Set AIZOME_RENDER_NODE to the GPU render node}"
aizome_bin=${AIZOME_BIN:-./result-aizome-gaming/bin/aizome}
gamescope_bin=${GAMESCOPE_BIN:-./result-gamescope-gaming/bin/gamescope}
gpu_index=${GAMING_GPU_INDEX:-0}
smoke_dir=$(mktemp -d /tmp/sekka-gaming-gpu.XXXXXX)
export XDG_RUNTIME_DIR="$smoke_dir"
export WAYLAND_DISPLAY=sekka-gaming-test
export XDG_CURRENT_DESKTOP=Aizome
unset WAYLAND_SOCKET DISPLAY
comp='' satellite=''
cleanup() {
  local status=$?
  if [[ -n "$satellite" ]]; then kill "$satellite" 2>/dev/null || true; wait "$satellite" 2>/dev/null || true; fi
  if [[ -n "$comp" ]]; then kill "$comp" 2>/dev/null || true; wait "$comp" 2>/dev/null || true; fi
  printf 'GPU smoke logs: %s\n' "$smoke_dir"
  if (( status != 0 )); then tail -n 30 "$smoke_dir"/*.log; fi
}
trap cleanup EXIT

# Xwayland, through satellite, forwards a Vulkan client's acquire and
# release points to the compositor.
xwayland_explicit_sync() {
  # Leave every existing X server alone. Satellite itself arbitrates the
  # selected display's lock if another process wins the race to start it.
  local display_number='' candidate n
  for candidate in $(seq 150 199); do
    if [[ ! -e /tmp/.X11-unix/X$candidate && ! -e /tmp/.X$candidate-lock ]]; then
      display_number=$candidate
      break
    fi
  done
  : "${display_number:?No unused test display found}"
  export DISPLAY=:$display_number
  WAYLAND_DEBUG=1 xwayland-satellite "$DISPLAY" >"$smoke_dir/satellite.log" 2>&1 &
  satellite=$!
  for ((n=0;n<100;n++)); do
    [[ -S /tmp/.X11-unix/X$display_number ]] && break
    kill -0 "$satellite"
    sleep .1
  done
  sleep 1
  timeout 30s vkcube --wsi xcb --gpu_number "$gpu_index" --c 240 >"$smoke_dir/x11-client.log" 2>&1
  grep -q 'set_acquire_point' "$smoke_dir/satellite.log"
  grep -q 'set_release_point' "$smoke_dir/satellite.log"
  cat "$smoke_dir/x11-client.log"
  kill "$satellite"
  wait "$satellite" 2>/dev/null || true
  satellite=
  unset DISPLAY
}

# Explicit sync is offered by default only on nvidia-drm. Turn it on, as the
# GPU test does, so that every GPU exercises it.
printf '[performance]\nexplicit_sync = "on"\n' >"$smoke_dir/settings.toml"
"$aizome_bin" --backend headless --renderer gles --headless-output 1280x720@1 \
  --socket "$WAYLAND_DISPLAY" --no-shell --no-xwayland --no-session \
  --config "$smoke_dir/settings.toml" >"$smoke_dir/compositor.log" 2>&1 &
comp=$!
for ((n=0;n<100;n++)); do
  [[ -S "$smoke_dir/aizome.$WAYLAND_DISPLAY.sock" ]] && break
  kill -0 "$comp"
  sleep .1
done
if grep -q 'explicit synchronization available' "$smoke_dir/compositor.log"; then
  xwayland_explicit_sync
  checked='Xwayland explicit sync and five Gamescope render/exit runs passed.'
else
  # Any other answer than a driver without syncobj eventfds is a failure.
  grep -q 'cannot wait on syncobj eventfds' "$smoke_dir/compositor.log"
  echo 'This GPU cannot wait on syncobj eventfds; skipping the Xwayland explicit-sync check.'
  checked='Five Gamescope render/exit runs passed; explicit sync was not checked.'
fi

# Disable only this test's NVIDIA shader cache; do not delete the user's
# caches. Short runs exercise shutdown while pre-compilation is still busy.
for frames in 240 3 3 3 3; do
  __GL_SHADER_DISK_CACHE=0 timeout 40s "$gamescope_bin" --fullscreen -- \
    vkcube --wsi xcb --gpu_number "$gpu_index" --c "$frames" \
    >"$smoke_dir/gamescope-$frames-$RANDOM.log" 2>&1
done
kill -0 "$comp"
echo "$checked"
