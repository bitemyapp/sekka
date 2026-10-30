#!/usr/bin/env bash
set -euo pipefail
cd /home/callen/work/hotaru-sprites-braille-20261028
export PATH=/home/callen/.cargo/bin:$PATH
(
    cd oracle-expanded
    python3 - <<'PY'
import json, subprocess
command=json.load(open('build-command.json'))
command=[x.replace('ReleaseSafe','ReleaseFast').replace('libsprites.so','libsprites-fast.so') for x in command]
subprocess.run(command,check=True)
PY
)
cargo +1.95.0 build --release --manifest-path rust/Cargo.toml >artifacts/build.log 2>&1
cargo +1.95.0 test --release --manifest-path rust/Cargo.toml >artifacts/tests-release.log 2>&1
cargo +1.95.0 clippy --all-targets --manifest-path rust/Cargo.toml -- -D warnings >artifacts/clippy.log 2>&1
