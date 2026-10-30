#!/usr/bin/env python3
"""Sweep CSI commands over parameter counts against the pinned library.

Usage: sweep.py REPLAY OUT.json
For every final byte 0x40-0x7E, private marker and intermediate, sends the
command with 0-4 parameters from six value sets on a screen of text: once with
the cursor away from home, and once with a wrap pending at the right margin
(reported with a "wrap:" prefix). It then queries the cursor, SGR, margins and
cursor style, and walks the tab stops, and reports which (private,
intermediate, final, parameters) differ.
"""
import json
import subprocess
import sys
import tempfile

sys.path.insert(0, "/home/callen/work/sekka-hotaru-settings/apps/hotaru/tools")
from differential import Oracle, differences  # noqa: E402

oracle = Oracle("/home/callen/work/ghostty-reference-20261028/zig-out/lib/libghostty-vt.so")
replay, out_path = sys.argv[1], sys.argv[2]
ROWS, COLS = 6, 20
SETUPS = {"": b"abcdefghijkl\r\nmnopqrstuvwx\r\nABCDEFGHIJKL\r\nMNOPQRSTUVWX\x1b[3;5H",
          "wrap:": b"abcdefghijkl\r\nmnopqrstuvwx\r\nABCDEFGHIJKL\r\nMNOPQRSTUVWX\x1b[3;1H" + b"x" * COLS}
QUERY = (b"Z\x1b[6n\x1bP$qm\x1b\\\x1bP$qr\x1b\\\x1bP$q q\x1b\\"
         b"\x1b[2;1H\t\x1b[6n\t\x1b[6n\t\x1b[6n")
VALUE_SETS = ([b"2", b"3", b"4", b"1"], [b"3", b"1", b"2", b"4"], [b"6", b"2", b"1", b"3"],
              [b"5", b"2", b"3", b"1"], [b"0", b"0", b"0", b"0"], [b"1", b"1", b"1", b"1"])
cases = []
for tag, setup in SETUPS.items():
    for private in (b"", b"?", b">", b"="):
        for intermediate in (b"", b" ", b"$", b"!", b"\"", b"'", b"*"):
            for final in range(0x40, 0x7F):
                for values in VALUE_SETS:
                    for count in range(1 if values is not VALUE_SETS[0] else 0, 5):
                        params = b";".join(values[:count])
                        body = b"\x1b[" + private + params + intermediate + bytes([final])
                        cases.append(((tag + private.decode(), intermediate.decode(), chr(final),
                                       params.decode()), setup + body + QUERY))

found = {}
with tempfile.TemporaryDirectory(dir="/dev/shm") as directory:
    for start in range(0, len(cases), 256):
        batch = cases[start:start + 256]
        for i, (key, data) in enumerate(batch):
            with open(f"{directory}/{i}.vt", "wb") as f:
                f.write(data)
        for i, (key, data) in enumerate(batch):
            out = subprocess.run([replay, "--data", f"{directory}/{i}.vt", "--rows", str(ROWS), "--cols", str(COLS),
                                  "--format", "json"], capture_output=True, text=True, check=True)
            delta = differences(oracle.snapshot(data, ROWS, COLS, 65536), json.loads(out.stdout))
            if delta:
                found.setdefault("".join(key[:3]), {})[key[3]] = delta[:3]
with open(out_path, "w") as f:
    json.dump(found, f, indent=1, ensure_ascii=False)
for command, counts in sorted(found.items()):
    print(repr(command), sorted(counts), json.dumps(next(iter(counts.values())))[:160])
print(len(cases), "cases,", len(found), "commands differ", file=sys.stderr)
