#!/usr/bin/env bash
# KTL-04, integration: the real start_write worker and elevate::run, with
# /run/wrappers/bin/pkexec masked by bwrap with a fake that copies the plan
# it is given to $KK_REPRO_OUT and exits 1. The real pkexec and the helper
# are never run; no device is opened; no network.
# Run from the repository root, in nix develop .#kakehashi, with bubblewrap:
#   bash apps/kakehashi/app/repro_ktl_04.sh
set -euo pipefail
bwrap=${BWRAP:-$(command -v bwrap)} || { echo "needs bubblewrap (bwrap), or BWRAP set to it" >&2; exit 1; }
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir "$work/out"
cat >"$work/pkexec" <<'EOF'
#!/bin/sh
# KAKEHASHI-REPRO-FAKE-PKEXEC (KTL-04): stands in for pkexec; never runs the helper.
# Arguments: HELPER --plan PLAN --progress PROGRESS
cp "$3" "$KK_REPRO_OUT/plan.json"
exit 1
EOF
chmod 755 "$work/pkexec"
manifest=apps/kakehashi/Cargo.toml
cargo test --locked --offline --manifest-path "$manifest" -p kakehashi --bin kakehashi --no-run
KK_REPRO_OUT="$work/out" "$bwrap" --dev-bind / / --tmpfs /run/wrappers --dir /run/wrappers/bin \
  --ro-bind "$work/pkexec" /run/wrappers/bin/pkexec --unshare-net -- \
  cargo test --locked --offline --manifest-path "$manifest" -p kakehashi --bin kakehashi repro_ktl_04 -- --include-ignored
