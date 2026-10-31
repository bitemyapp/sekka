#!/usr/bin/env bash
set -euo pipefail
cd /home/callen/work/hotaru-gpu-preparation-profile
mkdir -p artifacts
export PATH=/home/callen/.cargo/bin:$PATH
export AIZOME_FONT_DIR=/nix/store/b9vp4qd3gz9vmsl20grvd80hniirn5n5-aizome-fonts
export HOTARU_FONT_DIR=/nix/store/1rv68ixfzh8ki0jmanx7y846h2qdvzbg-hotaru-fonts
export CARGO_TARGET_DIR=/home/callen/.cache/hotaru-aizome-target
export HOTARU_SCENE_INVENTORY=$PWD/artifacts/profile.json
export HOTARU_SCENE_PROFILE=1
export HOTARU_SCENE_EXPECTED_DIR=/home/callen/work/hotaru-gpu-scene-isolated/artifacts/gpu-scene/fixtures
cargo +1.95.0 test --release --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-hotaru --test scene_inventory -- --nocapture > artifacts/profile.log 2>&1
