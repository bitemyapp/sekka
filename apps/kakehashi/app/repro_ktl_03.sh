#!/usr/bin/env bash
# KTL-03, integration: the real start_write worker and elevate::run, with
# /run/wrappers/bin/pkexec masked by bwrap with a fake that reports Writing
# and then dies by SIGKILL, as a helper killed mid-write does. The real
# pkexec and the helper are never run; no device is opened; no network.
# Run from the repository root, in nix develop .#kakehashi, with bubblewrap:
#   bash apps/kakehashi/app/repro_ktl_03.sh
set -euo pipefail
bwrap=${BWRAP:-$(command -v bwrap)} || { echo "needs bubblewrap (bwrap), or BWRAP set to it" >&2; exit 1; }
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
cat >"$work/pkexec" <<'EOF'
#!/bin/sh
# KAKEHASHI-REPRO-FAKE-PKEXEC (KTL-03): stands in for pkexec; never runs the helper.
# Arguments: HELPER --plan PLAN --progress PROGRESS
printf '%s\n' '{"stage":"preparing"}' '{"stage":"writing","done":1073741824,"total":4294967296}' >>"$5"
kill -KILL $$
EOF
chmod 755 "$work/pkexec"
manifest=apps/kakehashi/Cargo.toml
cargo test --locked --offline --manifest-path "$manifest" -p kakehashi --bin kakehashi --no-run
"$bwrap" --dev-bind / / --tmpfs /run/wrappers --dir /run/wrappers/bin \
  --ro-bind "$work/pkexec" /run/wrappers/bin/pkexec --unshare-net -- \
  cargo test --locked --offline --manifest-path "$manifest" -p kakehashi --bin kakehashi repro_ktl_03 -- --include-ignored
