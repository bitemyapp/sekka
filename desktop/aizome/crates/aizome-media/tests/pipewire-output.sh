#!/usr/bin/env bash
# A private sound server with two null sinks. WirePlumber's policy-only
# profile never loads hardware monitors or changes the desktop's sound.
set -euo pipefail
if [[ ${1:-} != --session ]]; then
    media_test_dir=$(mktemp -d -t utsushi-output.XXXXXX)
    trap 'rm -rf -- "$media_test_dir"' EXIT
    # Set these before starting D-Bus too: activated helpers must inherit
    # the private runtime/config/state, not the desktop session's paths.
    export XDG_RUNTIME_DIR="$media_test_dir/runtime"
    export PIPEWIRE_RUNTIME_DIR="$XDG_RUNTIME_DIR"
    export PIPEWIRE_REMOTE=pipewire-0
    export XDG_CONFIG_HOME="$media_test_dir/config"
    export XDG_STATE_HOME="$media_test_dir/state"
    export XDG_CONFIG_DIRS="$media_test_dir/system-config"
    export GIO_USE_VFS=local
    mkdir -p "$XDG_RUNTIME_DIR" "$XDG_CONFIG_HOME" "$XDG_STATE_HOME"
    chmod 700 "$XDG_RUNTIME_DIR"
    dbus-run-session -- bash "$0" --session "$media_test_dir"
    exit
fi
media_manifest=$(cd -- "$(dirname -- "$0")/.." && pwd)/Cargo.toml
media_test_dir=$2
media_pipewire_pid=
media_wireplumber_pid=
cleanup() {
    status=$?
    if [[ $status != 0 ]]; then
        cat "$media_test_dir/pipewire.log" "$media_test_dir/wireplumber.log" >&2 || true
    fi
    [[ -z $media_wireplumber_pid ]] || kill "$media_wireplumber_pid" 2>/dev/null || true
    [[ -z $media_pipewire_pid ]] || kill "$media_pipewire_pid" 2>/dev/null || true
    wait || true
}
trap cleanup EXIT
export XDG_RUNTIME_DIR="$media_test_dir/runtime"
export PIPEWIRE_RUNTIME_DIR="$XDG_RUNTIME_DIR"
export PIPEWIRE_REMOTE=pipewire-0
export XDG_CONFIG_HOME="$media_test_dir/config"
export XDG_STATE_HOME="$media_test_dir/state"
export XDG_CONFIG_DIRS="$media_test_dir/system-config"
export AIZOME_MEDIA_TEST_PIPEWIRE=1
export AIZOME_MEDIA_TEST_AUDIO="$media_test_dir/tone.wav"
mkdir -p "$XDG_RUNTIME_DIR" "$XDG_CONFIG_HOME/pipewire/pipewire.conf.d" "$XDG_STATE_HOME"
chmod 700 "$XDG_RUNTIME_DIR"
cat > "$XDG_CONFIG_HOME/pipewire/pipewire.conf.d/91-test.conf" <<'EOF'
context.objects = [
  { factory = adapter args = {
      factory.name = support.null-audio-sink
      node.name = utsushi-test-a
      node.description = "Test Speakers"
      media.class = Audio/Sink
      audio.position = [ FL FR ]
      object.linger = true
      priority.session = 1009
      priority.driver = 1009
  } }
  { factory = adapter args = {
      factory.name = support.null-audio-sink
      node.name = utsushi-test-b
      node.description = "Test Headphones"
      media.class = Audio/Sink
      audio.position = [ FL FR ]
      object.linger = true
      priority.session = 1008
      priority.driver = 1008
  } }
]
EOF
ffmpeg -nostdin -hide_banner -loglevel error -f lavfi -i "sine=frequency=440:sample_rate=48000:duration=30" -ac 2 "$AIZOME_MEDIA_TEST_AUDIO"
pipewire > "$media_test_dir/pipewire.log" 2>&1 &
media_pipewire_pid=$!
for _ in {1..100}; do
    [[ ! -S $XDG_RUNTIME_DIR/pipewire-0 ]] || break
    sleep 0.05
done
wireplumber --profile=policy > "$media_test_dir/wireplumber.log" 2>&1 &
media_wireplumber_pid=$!
cargo test --locked --manifest-path "$media_manifest" -p aizome-utsushi --test outputs -- --ignored --test-threads=1
