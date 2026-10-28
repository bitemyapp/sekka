#!/usr/bin/env bash
set -euo pipefail
cd /home/callen/work/hotaru-native-braille-20261028
export PATH=/home/callen/.cargo/bin:$PATH
export CARGO_TARGET_DIR=/home/callen/.cache/hotaru-aizome-target
export AIZOME_FONT_DIR=/nix/store/b9vp4qd3gz9vmsl20grvd80hniirn5n5-aizome-fonts
export HOTARU_FONT_DIR=/nix/store/1rv68ixfzh8ki0jmanx7y846h2qdvzbg-hotaru-fonts
cargo +1.95.0 build --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-hotaru >artifacts/braille/native-build-final.log 2>&1
cp "$CARGO_TARGET_DIR/debug/aizome-hotaru" artifacts/braille/hotaru
python3 desktop/aizome/apps/hotaru/tests/wayland/sprites.py artifacts/braille/hotaru /home/callen/work/hotaru-sprites-braille-20261028/oracle-expanded/libsprites-fast.so artifacts/braille/wayland >artifacts/braille/wayland.log 2>&1
