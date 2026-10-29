#!/usr/bin/env bash
set -euo pipefail
cd /home/callen/work/hotaru-semantic-prompts-20261028
exec 9>/home/callen/.cache/hotaru-workstation.lock
flock -x 9
export PATH=/home/callen/.cargo/bin:$PATH
export CARGO_TARGET_DIR=/home/callen/.cache/hotaru-semantic-target
cargo +1.95.0 fmt --manifest-path apps/hotaru/Cargo.toml --all
cargo +1.95.0 test --locked --manifest-path apps/hotaru/Cargo.toml >artifacts/debug-tests.log 2>&1
cargo +1.95.0 test --release --locked --manifest-path apps/hotaru/Cargo.toml >artifacts/release-tests.log 2>&1
cargo +1.95.0 clippy --locked --manifest-path apps/hotaru/Cargo.toml --all-targets -- -D warnings >artifacts/clippy.log 2>&1
MIRIFLAGS=-Zmiri-strict-provenance cargo +nightly miri test --locked --manifest-path apps/hotaru/Cargo.toml -p hotaru-core --lib row::ascii::tests >artifacts/miri-ascii.log 2>&1
