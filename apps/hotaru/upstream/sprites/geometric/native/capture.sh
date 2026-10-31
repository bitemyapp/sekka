#!/usr/bin/env bash
set -euo pipefail
cd /home/callen/work/hotaru-native-geometric-20261028
export PATH=/home/callen/.cargo/bin:$PATH
export CARGO_TARGET_DIR=/home/callen/.cache/hotaru-aizome-target
export AIZOME_FONT_DIR=/nix/store/b9vp4qd3gz9vmsl20grvd80hniirn5n5-aizome-fonts
export HOTARU_FONT_DIR=/nix/store/1rv68ixfzh8ki0jmanx7y846h2qdvzbg-hotaru-fonts
python3 artifacts/geometric/manifest.py
python3 desktop/aizome/apps/hotaru/tests/wayland/sprites.py artifacts/geometric/hotaru /home/callen/work/hotaru-sprites-geometric-20261028/oracle-expanded/libsprites-fast.so artifacts/geometric/wayland >artifacts/geometric/wayland.log 2>&1
