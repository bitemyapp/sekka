#!/usr/bin/env bash
set -euo pipefail
cd /home/callen/work/hotaru-semantic-fastgate-20261028
exec 9>/home/callen/.cache/hotaru-workstation.lock
flock -x 9
export PATH=/home/callen/.cargo/bin:$PATH
export CARGO_TARGET_DIR=/home/callen/.cache/hotaru-semantic-fastgate-target
rustup run 1.95.0 rustfmt --edition 2024 apps/hotaru/crates/hotaru-core/src/screen.rs apps/hotaru/crates/hotaru-core/tests/semantic_fastgate.rs
cargo +1.95.0 test --locked --release --manifest-path apps/hotaru/Cargo.toml -p hotaru-core --test semantic --test decaln --test semantic_fastgate >artifacts/targeted-release.log 2>&1
cargo +1.95.0 clippy --locked --manifest-path apps/hotaru/Cargo.toml --workspace --all-targets -- -D warnings >artifacts/clippy.log 2>&1
cargo +1.95.0 build --locked --manifest-path apps/hotaru/Cargo.toml -p hotaru-core --example semantic_capture >artifacts/adapter.log 2>&1
cp "$CARGO_TARGET_DIR/debug/examples/semantic_capture" artifacts/hotaru-semantic
cp /home/callen/work/hotaru-semantic-prompts-20261028/artifacts/ghostty-semantic artifacts/ghostty-semantic
sha256sum artifacts/hotaru-semantic artifacts/ghostty-semantic /home/callen/work/ghostty-reference-20261028/zig-out/lib/libghostty-vt.so.0.1.0 >artifacts/adapter-sha256.txt
