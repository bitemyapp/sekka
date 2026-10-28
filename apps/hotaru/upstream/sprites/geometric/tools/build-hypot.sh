#!/usr/bin/env bash
set -euo pipefail
cd /home/callen/work/hotaru-sprites-geometric-20261028
export PATH=/home/callen/.cargo/bin:$PATH
if [ ! -e artifacts/initial-raster ]; then cp rust/target/release/hotaru-sprite-prototype artifacts/initial-raster; fi
cargo +1.95.0 build --release --manifest-path rust/Cargo.toml --example hypot --bin hotaru-sprite-prototype >artifacts/hypot-build.log 2>&1
/home/callen/.cache/hotaru-reference/zig-x86_64-linux-0.17.0/zig build-lib -dynamic -lc -O ReleaseSafe tools/hypot.zig -femit-bin=artifacts/libhypot.so
cargo +1.95.0 test --release --manifest-path rust/Cargo.toml >artifacts/hypot-tests.log 2>&1
cargo +1.95.0 clippy --all-targets --manifest-path rust/Cargo.toml -- -D warnings >artifacts/hypot-clippy.log 2>&1
