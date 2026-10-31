#!/usr/bin/env bash
# fmt, clippy and aizome-hotaru tests for the gather change, in the main-tree dev shell.
set -uo pipefail
exec 9>/home/callen/.cache/hotaru-workstation.lock
flock -x -w 1800 9
source <(grep -v '^eval .${shellHook' /home/callen/sekka-work/rc-hotaru/env.sh)
export RUSTC_WRAPPER=/dev/shm/gui-tp-rustc-wrapper CARGO_TARGET_DIR=/dev/shm/gui-tp-target CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
cd /home/callen/work/sekka-gui-throughput/desktop/aizome
cargo fmt --check -p aizome-hotaru && echo "fmt: clean" || echo "fmt: FAILED"
cargo clippy --locked -p aizome-hotaru --all-targets 2>&1 | grep -E "^(warning|error)" -A6 | grep -v fetch_update | head -30
cargo test --locked -p aizome-hotaru --no-fail-fast 2>&1 | grep -E "test result|FAILED|panicked|^error" | sed 's/finished in [0-9.]*s//' | sort | uniq -c
