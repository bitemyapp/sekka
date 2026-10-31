#!/usr/bin/env python3
"""Compare DECRQM replies for every ANSI mode below 100 and DEC mode below
10,000, before and after setting each, between hotaru-replay and the pinned
library. Usage: modes.py REPLAY"""
import json
import re
import subprocess
import sys
import tempfile

sys.path.insert(0, "/home/callen/work/sekka-hotaru-settings/apps/hotaru/tools")
from differential import Oracle  # noqa: E402

oracle = Oracle("/home/callen/work/ghostty-reference-20261028/zig-out/lib/libghostty-vt.so")
replay = sys.argv[1]


def replies(data):
    expected = bytes(oracle.snapshot(data, 4, 20, 65536)["responses"])
    with tempfile.NamedTemporaryFile(dir="/dev/shm") as f:
        f.write(data)
        f.flush()
        out = subprocess.run([replay, "--data", f.name, "--rows", "4", "--cols", "20", "--format", "json"],
                             capture_output=True, text=True, check=True)
    return expected, bytes(json.loads(out.stdout)["responses"])


def parse(reply):
    return {int(m.group(1)): int(m.group(2)) for m in re.finditer(rb"\x1b\[\??(\d+);(\d+)\$y", reply)}


for ansi, limit in ((True, 100), (False, 10000)):
    prefix = "" if ansi else "?"
    for start in range(0, limit, 200):
        numbers = range(start, min(start + 200, limit))
        query = b"".join(f"\x1b[{prefix}{n}$p".encode() for n in numbers)
        for label, setup in (("initial", b""),):
            g, h = replies(setup + query)
            if g != h:
                g, h = parse(g), parse(h)
                for n in numbers:
                    if g.get(n) != h.get(n):
                        print("ansi" if ansi else "dec", n, "ghostty", g.get(n), "hotaru", h.get(n))
        # Setting each mode alone, then querying it, finds modes one side ignores.
        for n in numbers:
            data = f"\x1b[{prefix}{n}h\x1b[{prefix}{n}$p".encode()
            g, h = replies(data)
            if g != h:
                print("ansi" if ansi else "dec", n, "after set: ghostty", g, "hotaru", h)
