#!/usr/bin/env python3
"""Find the shortest prefix of an input after which hotaru-replay's screen
differs from the pinned libghostty-vt's, and show the bytes around it.

Usage: divergence.py REPLAY INPUT [--limit BYTES]
Prefixes grow geometrically until the screens differ, then a binary search
finds the first differing length. States can converge again later, so this
finds the first divergence, not every one.
"""
import json
import subprocess
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, "/home/callen/work/sekka-hotaru-settings/apps/hotaru/tools")
from differential import Oracle, differences  # noqa: E402

LIBRARY = "/home/callen/work/ghostty-reference-20261028/zig-out/lib/libghostty-vt.so"
ROWS, COLS = 80, 120
oracle = Oracle(LIBRARY)
replay = sys.argv[1]
data = Path(sys.argv[2]).read_bytes()
if "--limit" in sys.argv:
    data = data[:int(sys.argv[sys.argv.index("--limit") + 1])]


def differ(n):
    prefix = data[:n]
    expected = oracle.snapshot(prefix, ROWS, COLS, 65536)
    with tempfile.NamedTemporaryFile(dir="/dev/shm") as f:
        f.write(prefix)
        f.flush()
        out = subprocess.run([replay, "--data", f.name, "--rows", str(ROWS), "--cols", str(COLS), "--format", "json"],
                             capture_output=True, text=True, check=True)
    return differences(expected, json.loads(out.stdout))


low, high = 0, 64
while high < len(data) and not differ(high):
    low, high = high, min(high * 2, len(data))
if not differ(high):
    print(json.dumps({"diverges": False, "checked_bytes": len(data)}))
    sys.exit(0)
while high - low > 1:
    middle = (low + high) // 2
    if differ(middle):
        high = middle
    else:
        low = middle
print(json.dumps({"diverges": True, "first_differing_length": high,
                  "last_byte_hex": data[high - 1:high].hex(),
                  "context_hex": data[max(0, high - 96):high + 16].hex(),
                  "context_text": data[max(0, high - 96):high + 16].decode("utf-8", "backslashreplace"),
                  "differences": differ(high)[:8]}, indent=1, ensure_ascii=False))
