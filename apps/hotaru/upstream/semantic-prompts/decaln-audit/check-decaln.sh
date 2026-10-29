#!/usr/bin/env bash
set -euo pipefail
cd /home/callen/work/hotaru-semantic-prompts-20261028
exec 9>/home/callen/.cache/hotaru-workstation.lock
flock -x 9
export PATH=/home/callen/.cargo/bin:$PATH
export CARGO_TARGET_DIR=/home/callen/.cache/hotaru-semantic-target
rustup run 1.95.0 rustfmt --edition 2024 apps/hotaru/crates/hotaru-core/src/screen.rs apps/hotaru/crates/hotaru-core/tests/decaln.rs
cp artifacts/hotaru-semantic artifacts/hotaru-semantic-before-decaln
cp artifacts/adapter-sha256.txt artifacts/adapter-sha256-before-decaln.txt
cargo +1.95.0 build --locked --manifest-path apps/hotaru/Cargo.toml -p hotaru-core --example semantic_capture >artifacts/decaln-adapter.log 2>&1
cp "$CARGO_TARGET_DIR/debug/examples/semantic_capture" artifacts/hotaru-semantic
sha256sum artifacts/ghostty-semantic artifacts/hotaru-semantic /home/callen/work/ghostty-reference-20261028/zig-out/lib/libghostty-vt.so.0.1.0 >artifacts/adapter-sha256.txt
cargo +1.95.0 test --locked --manifest-path apps/hotaru/Cargo.toml --workspace >artifacts/decaln-workspace.log 2>&1
cargo +1.95.0 clippy --locked --manifest-path apps/hotaru/Cargo.toml --workspace --all-targets -- -D warnings >artifacts/decaln-clippy.log 2>&1
