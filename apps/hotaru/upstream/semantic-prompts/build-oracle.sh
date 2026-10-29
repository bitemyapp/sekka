#!/usr/bin/env bash
set -euo pipefail
cd /home/callen/work/hotaru-semantic-prompts-20261028
exec 9>/home/callen/.cache/hotaru-workstation.lock
flock -x 9
export PATH=/home/callen/.cargo/bin:$PATH
export CARGO_TARGET_DIR=/home/callen/.cache/hotaru-semantic-target
ref=/home/callen/work/ghostty-reference-20261028
cc -std=c11 -Wall -Wextra -Werror -O2 -I"$ref/include" artifacts/oracle.c -L"$ref/zig-out/lib" -Wl,-rpath,"$ref/zig-out/lib" -lghostty-vt -o artifacts/ghostty-semantic
cargo +1.95.0 fmt --manifest-path apps/hotaru/Cargo.toml --all
cargo +1.95.0 build --locked --manifest-path apps/hotaru/Cargo.toml -p hotaru-core --example semantic_capture >artifacts/adapter-build.log 2>&1
cp "$CARGO_TARGET_DIR/debug/examples/semantic_capture" artifacts/hotaru-semantic
sha256sum artifacts/ghostty-semantic artifacts/hotaru-semantic "$ref/zig-out/lib/libghostty-vt.so.0.1.0" >artifacts/adapter-sha256.txt
