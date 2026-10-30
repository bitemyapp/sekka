#!/usr/bin/env bash
# Run through ssh wx-workstation; do not run builds/tests on the laptop.
set -euo pipefail
record=$(dirname "$(realpath "$0")")
source_root=$(realpath "$1")
evidence=$(realpath -m "$2")
mkdir -p "$evidence"
exec 9>/home/callen/.cache/hotaru-workstation.lock
flock -x 9
python3 - "$source_root" "$record/source-manifest.json" <<'PY'
import hashlib, json, sys
from pathlib import Path
root, manifest = map(Path, sys.argv[1:])
for name, expected in json.loads(manifest.read_text())["sources"].items():
    assert hashlib.sha256((root / name).read_bytes()).hexdigest() == expected, name
PY
cd "$source_root"
export PATH=/home/callen/.cargo/bin:$PATH
export CARGO_TARGET_DIR=/home/callen/.cache/hotaru-aizome-target
export AIZOME_FONT_DIR=/nix/store/b9vp4qd3gz9vmsl20grvd80hniirn5n5-aizome-fonts
export HOTARU_FONT_DIR=/nix/store/1rv68ixfzh8ki0jmanx7y846h2qdvzbg-hotaru-fonts
cargo +1.95.0 test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-hotaru -- --nocapture >"$evidence/tests.log" 2>&1
cargo +1.95.0 clippy --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-hotaru --all-targets --no-deps -- -D warnings >"$evidence/clippy.log" 2>&1
cargo +1.95.0 build --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-hotaru >"$evidence/build.log" 2>&1
cp "$CARGO_TARGET_DIR/debug/aizome-hotaru" "$evidence/hotaru"
sha256sum "$evidence/hotaru" >"$evidence/binary.sha256"
python3 desktop/aizome/apps/hotaru/tests/wayland/pixel_scroll.py "$evidence/hotaru" "$evidence/wayland" >"$evidence/wayland.log" 2>&1
