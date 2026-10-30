#!/usr/bin/env python3
"""Compare every code point's printed width in hotaru-replay with the pinned
libghostty-vt's width table, and print the mismatches as ranges.

Usage: widths.py REPLAY OUT.json
Each code point is printed after an "A" on its own line; Hotaru's width is read
back from the replay's cells (0 when it joins or drops the code point).
"""
import ctypes as C
import json
import subprocess
import sys
import tempfile
import unicodedata

LIBRARY = "/home/callen/work/ghostty-reference-20261028/zig-out/lib/libghostty-vt.so"
lib = C.CDLL(LIBRARY)
lib.ghostty_unicode_codepoint_width.argtypes = [C.c_uint32]
lib.ghostty_unicode_codepoint_width.restype = C.c_uint8
replay, out_path = sys.argv[1], sys.argv[2]

points = [cp for cp in range(0xA0, 0x110000) if not 0xD800 <= cp <= 0xDFFF]
mismatches = []
for start in range(0, len(points), 4095):
    batch = points[start:start + 4095]
    data = b"".join(b"A" + chr(cp).encode() + b"\r\n" for cp in batch)
    with tempfile.NamedTemporaryFile(dir="/dev/shm") as f:
        f.write(data)
        f.flush()
        out = subprocess.run([replay, "--data", f.name, "--rows", str(len(batch) + 1), "--cols", "4",
                              "--format", "json"], capture_output=True, text=True, check=True)
    lines = json.loads(out.stdout)["lines"]
    for cp, line in zip(batch, lines):
        cells = line["cells"]
        if cells[0]["text"] != "A":
            hotaru = 0
        elif cells[1]["text"] == chr(cp):
            hotaru = cells[1]["width"]
        else:
            hotaru = 0
        ghostty = lib.ghostty_unicode_codepoint_width(cp)
        if hotaru != ghostty:
            mismatches.append((cp, ghostty, hotaru))
    if start % (4095 * 32) == 0:
        print(f"through U+{batch[-1]:04X}: {len(mismatches)} mismatches", file=sys.stderr)

ranges = []
for cp, g, h in mismatches:
    category = unicodedata.category(chr(cp))
    if ranges and ranges[-1]["last"] == cp - 1 and ranges[-1]["ghostty"] == g and ranges[-1]["hotaru"] == h \
            and ranges[-1]["category"] == category:
        ranges[-1]["last"] = cp
    else:
        ranges.append({"first": cp, "last": cp, "ghostty": g, "hotaru": h, "category": category})
with open(out_path, "w") as f:
    json.dump({"unicodedata": unicodedata.unidata_version, "count": len(mismatches),
               "ranges": [dict(r, first=f"U+{r['first']:04X}", last=f"U+{r['last']:04X}",
                               name=unicodedata.name(chr(r["first"]), "")) for r in ranges]}, f, indent=1)
print(len(mismatches), "mismatches in", len(ranges), "ranges")
