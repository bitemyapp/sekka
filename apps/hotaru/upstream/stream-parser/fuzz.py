#!/usr/bin/env python3
"""Compare short random control-heavy byte strings between hotaru-replay and
the pinned libghostty-vt, and minimize each differing case.

Usage: fuzz.py REPLAY CASES SEED
"""
import json
import random
import subprocess
import sys
import tempfile

sys.path.insert(0, "/home/callen/work/sekka-hotaru-settings/apps/hotaru/tools")
from differential import Oracle, differences  # noqa: E402

oracle = Oracle("/home/callen/work/ghostty-reference-20261028/zig-out/lib/libghostty-vt.so")
replay, cases, seed = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])
rng = random.Random(seed)
ROWS, COLS = 6, 20
PIECES = [b"\x1b", b"[", b"]", b"P", b"X", b"^", b"_", b"\\", b"\x07", b"\x18", b"\x1a", b"$", b"q", b"?",
          b";", b":", b"0", b"1", b"2", b"5", b"8", b"m", b"H", b"J", b"K", b"r", b"h", b"l", b" ", b"a",
          b"\x7f", b"\r", b"\n", b"\x08", b"\x09", b"\x0e", b"\x0f", b"(", b"0", b"#", b"\xc3\xa9",
          b"\xe4\xb8\x80", b"\xf0\x9f\x98\x80", b"\xcc\x81", b"\xe2\x80\x8d",
          b"+", b">", b"<", b"=", b"!", b"\"", b"'", b"4", b"6", b"n", b"c", b"t", b"p", b"@", b"\x00", b"\x05", b"\x0b",
          b"\x0c", b"\x1c", b"6", b"7", b"D", b"E", b"M", b"Z", b"\xe3\x80\xaf"]
C1 = [bytes([b]) for b in range(0x80, 0xa0)] + [bytes([b]) for b in (0xa0, 0xc3, 0xe4, 0xf0, 0xff)]


def case():
    out = []
    for _ in range(rng.randint(1, 40)):
        out.append(rng.choice(C1) if rng.random() < 0.15 else rng.choice(PIECES))
    return b"".join(out)


def differ(data):
    expected = oracle.snapshot(data, ROWS, COLS, 65536)
    with tempfile.NamedTemporaryFile(dir="/dev/shm") as f:
        f.write(data)
        f.flush()
        out = subprocess.run([replay, "--data", f.name, "--rows", str(ROWS), "--cols", str(COLS), "--format", "json"],
                             capture_output=True, text=True, check=True)
    return differences(expected, json.loads(out.stdout))


def minimize(data):
    size = max(1, len(data) // 2)
    while size >= 1:
        i = 0
        while i < len(data):
            trial = data[:i] + data[i + size:]
            if trial and differ(trial):
                data = trial
            else:
                i += size
        size //= 2
    return data


seen = set()
for n in range(cases):
    data = case()
    if differ(data):
        small = minimize(data)
        if small not in seen:
            seen.add(small)
            print(json.dumps({"case": small.hex(), "differences": differ(small)[:4]}, ensure_ascii=False))
print(f"{cases} cases, {len(seen)} distinct minimized differences", file=sys.stderr)
