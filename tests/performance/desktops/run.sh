#!/usr/bin/env bash
# Run bench.py with the host tools of default.nix (QEMU, erofs-utils,
# e2fsprogs, Python with NumPy). See docs/performance/desktops.md.
#
#   run.sh setup VARIANT
#   run.sh trial VARIANT TAG [--mem MIB] [--phases ...] [--png]
#   run.sh summarize
set -euo pipefail
here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
repo=$(cd "$here/../../.." && pwd)
host=$(cd "$repo" && nix build --impure -f "$here" --no-link --print-out-paths host 2>/dev/null)
export PATH=$host/bin:$PATH
if [ "${1:-}" = summarize ]; then
  shift
  exec python3 "$here/summarize.py" "$@"
fi
exec python3 "$here/bench.py" "$@"
