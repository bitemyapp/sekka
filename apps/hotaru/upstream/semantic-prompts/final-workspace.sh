#!/usr/bin/env bash
set -euo pipefail
cd /home/callen/work/hotaru-semantic-fastgate-20261028
exec 9>/home/callen/.cache/hotaru-workstation.lock
flock -x 9
export PATH=/home/callen/.cargo/bin:$PATH
export CARGO_TARGET_DIR=/home/callen/.cache/hotaru-semantic-fastgate-target
cargo +1.95.0 fmt --manifest-path apps/hotaru/Cargo.toml --all --check >artifacts/final-fmt.log 2>&1
cargo +1.95.0 test --locked --manifest-path apps/hotaru/Cargo.toml --workspace >artifacts/final-debug.log 2>&1
cargo +1.95.0 test --locked --release --manifest-path apps/hotaru/Cargo.toml --workspace >artifacts/final-release.log 2>&1
printf 'Final fastgate format, debug and release workspace checks passed\n'

