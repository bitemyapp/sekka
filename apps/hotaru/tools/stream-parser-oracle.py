#!/usr/bin/env python3
"""Pinned-library stream parser and scalar width comparisons.

Covers the rules that Hotaru broke and that three sources found: Ghostty's
random UTF-8 and OSC generators, a seeded control-heavy fuzzer and a sweep of
CSI parameter counts. The rules:

- DEL in ground state.
- Raw C1 controls outside OSC and DCS payloads.
- DECRQSS request length and payload controls.
- DCS header controls.
- Parameter, colon and private-marker limits.
- Escape intermediates.
- Commands with more parameters than they take, and explicit zero counts.
- HPB, VPB, CTC and DECST8C.
- A pending wrap across scrolling.
- The per-scalar width table, in both width modes.

Uses differential.py's checks and options; expected data comes only from the
actual pinned library. --compact-fixtures writes the Cargo test's fixture.
"""
import argparse
import hashlib
import json
from pathlib import Path
import random
import sys

import differential


# Every scalar whose printed width differed from the reference's table while
# Hotaru's legacy mode used the unicode-width crate (inclusive ranges).
WIDTH_RANGES = (
    (0x0605, 0x0605), (0x070F, 0x070F), (0x0890, 0x0891), (0x08E2, 0x08E2), (0x09BE, 0x09BE),
    (0x09D7, 0x09D7), (0x0B3E, 0x0B3E), (0x0B57, 0x0B57), (0x0BBE, 0x0BBE), (0x0BD7, 0x0BD7),
    (0x0CC0, 0x0CC0), (0x0CC2, 0x0CC2), (0x0CC7, 0x0CC8), (0x0CCA, 0x0CCB), (0x0CD5, 0x0CD6),
    (0x0D3E, 0x0D3E), (0x0D4E, 0x0D4E), (0x0D57, 0x0D57), (0x0DCF, 0x0DCF), (0x0DDF, 0x0DDF),
    (0x115F, 0x115F), (0x1715, 0x1715), (0x1734, 0x1734), (0x17A4, 0x17A4), (0x17D8, 0x17D8),
    (0x1B35, 0x1B35), (0x1B3B, 0x1B3B), (0x1B3D, 0x1B3D), (0x1B43, 0x1B44), (0x1BAA, 0x1BAA),
    (0x1BF2, 0x1BF3), (0x2028, 0x2029), (0x2D7F, 0x2D7F), (0x2E3A, 0x2E3B), (0x302E, 0x302F),
    (0xA8FA, 0xA8FA), (0xA953, 0xA953), (0xA9C0, 0xA9C0), (0x111C0, 0x111C0), (0x111C2, 0x111C3),
    (0x11235, 0x11235), (0x1133E, 0x1133E), (0x1134D, 0x1134D), (0x11357, 0x11357),
    (0x113B8, 0x113B8), (0x113C2, 0x113C2), (0x113C5, 0x113C5), (0x113C7, 0x113C9),
    (0x113CF, 0x113CF), (0x113D1, 0x113D1), (0x114B0, 0x114B0), (0x114BD, 0x114BD),
    (0x115AF, 0x115AF), (0x116B6, 0x116B6), (0x11930, 0x11930), (0x1193D, 0x1193D),
    (0x1193F, 0x1193F), (0x11941, 0x11941), (0x11A84, 0x11A89), (0x11D46, 0x11D46),
    (0x11F02, 0x11F02), (0x11F41, 0x11F41), (0x16D63, 0x16D63), (0x16D67, 0x16D6A),
    (0x16FF0, 0x16FF1), (0x1D165, 0x1D166), (0x1D16D, 0x1D172), (0x1D250, 0x1D252),
    (0x1D25F, 0x1D25F), (0x1D280, 0x1D281),
)

# Header states and string states a raw C1 byte can arrive in.
C1_CONTEXTS = {
    "escape": b"\x1b", "escape-intermediate": b"\x1b(", "csi-entry": b"\x1b[",
    "csi-param": b"\x1b[1;", "csi-intermediate": b"\x1b[1 ", "csi-ignore": b"\x1b[1?",
    "dcs-entry": b"\x1bP", "dcs-param": b"\x1bP1;", "dcs-payload": b"\x1bP$q",
    "dcs-ignore": b"\x1bP1:q", "osc": b"\x1b]0;", "sos": b"\x1bX", "pm": b"\x1b^", "apc": b"\x1b_",
}
C1_BYTES = (0x80, 0x84, 0x85, 0x88, 0x8D, 0x8E, 0x90, 0x98, 0x9A, 0x9B, 0x9C, 0x9D, 0x9E, 0x9F)

PIECES = [b"\x1b", b"[", b"]", b"P", b"X", b"^", b"_", b"\\", b"\x07", b"\x18", b"\x1a", b"$", b"q", b"?",
          b";", b":", b"0", b"1", b"2", b"5", b"8", b"m", b"H", b"J", b"K", b"r", b"h", b"l", b" ", b"a",
          b"\x7f", b"\r", b"\n", b"\x08", b"\x09", b"\x0e", b"\x0f", b"(", b"#", "é".encode(), "一".encode(),
          "😀".encode(), "́".encode(), "‍".encode(), b"+", b">", b"<", b"=", b"!", b"\"", b"'",
          b"4", b"6", b"n", b"c", b"t", b"p", b"@", b"\x00", b"\x05", b"\x0b", b"\x0c", b"\x1c", b"7", b"D",
          b"E", b"M", b"Z", "〯".encode()]
RAW = [bytes([byte]) for byte in (*range(0x80, 0xA0), 0xA0, 0xC3, 0xE4, 0xF0, 0xFF)]


def fixtures():
    cases = []

    def add(name, data, cols=10, rows=3):
        cases.append(dict(name=name, data=data, cols=cols, rows=rows))

    query = b"\x1b[6n"
    add("ground-del", b"A\x7fB" + query)
    add("ground-del-wrap", b"abcdefgh\x7f\x7fZ" + query)
    add("ground-del-combining", b"A\x7f\xcc\x81B" + query)
    for context, prefix in C1_CONTEXTS.items():
        for byte in C1_BYTES:
            add(f"c1-{context}-{byte:x}", b"\x1b[2;3HA" + prefix + bytes([byte]) + b"5mB\x1b\\C" + query)
    for context in ("escape", "csi-entry", "csi-param", "dcs-entry", "dcs-param"):
        for byte in (0xA0, 0xC3, 0xE9, 0xFF):
            add(f"high-{context}-{byte:x}", b"A" + C1_CONTEXTS[context] + bytes([byte]) + b"5mB\x1b\\C" + query)
    for request in (b"", b"m", b"r", b" q", b"s", b"zz", b"mm", b"zzz", b"mmm", b"m\n", b"\nm", b"\x05m",
                    b"m\x7f", b"\x7fm", b"r\x00", b"b\x91b", b"\x91", "é".encode(), b"b\xc3\xa9"):
        add(f"decrqss-{request.hex() or 'empty'}", b"\x1b[2;3r\x1b[1;31m\x1bP$q" + request + b"\x1b\\Z" + query)
    for i, header in enumerate((b"\n", b"\r", b"\x05", b"\x08", b"1\n", b"1:2", b":", b"0?", b"?", b"1;?",
                                b">", b"$\n", b"1;2", b";")):
        add(f"dcs-header-{i}", b"ab\x1bP" + header + b"$qm\x1b\\cd" + query)
    for i, sequence in enumerate((b"0?25l", b"?0?25l", b";?25l", b"1;?25l", b"?25l", b"0>c", b">c",
                                  b"0?6n", b"?6n")):
        add(f"csi-private-{i}", b"ab\x1b[" + sequence + b"Z" + query)
    for final in b"@ABCDEFGHJKLMPSTXZ`abcdefghlmnrsu":
        add(f"csi-colon-{final:x}", b"abc\r\nabc\r\nabc\x1b[2;2H\x1b[1:2" + bytes([final]) + b"X" + query)
    for i, sgr in enumerate((b"4:3", b"38:2::255:0:0", b"38:5:1", b"1;4:3;38:5:2", b"58:2::1:2:3;4:2",
                             b":", b"4:", b"1:2:3:4:5:6:7")):
        add(f"sgr-colon-{i}", b"\x1b[" + sgr + b"mA" + query)
    for count in (23, 24, 25, 26):
        ones = [b"1"] * (count - 1)
        add(f"sgr-parameters-{count}", b"\x1b[" + b";".join(ones + [b"31"]) + b"mA" + query)
        add(f"sgr-colon-parameters-{count}", b"\x1b[" + b":".join(ones + [b"3"]) + b"mA" + query)
        add(f"cup-parameters-{count}", b"\x1b[" + b";".join([b"2", b"3"] + [b"1"] * (count - 2)) + b"HA" + query)
        add(f"dcs-parameters-{count}", b"\x1bP" + b";".join([b"1"] * count) + b"$qm\x1b\\A" + query)
    # Commands ignore more parameters than they take; HPB and VPB are CUB
    # and CUU.
    screen = b"abcdefghijkl\r\nmnopqrstuvwx\r\nABCDEFGHIJKL\r\nMNOPQRSTUVWX\x1b[3;5H"
    for final in b"@ABCDEFGIJKLMPSTXZ`adegjkn":
        for params in (b"", b"3", b"0", b"3;2", b"0;3", b"6;1"):
            add(f"csi-count-{final:x}-{params.hex() or 'none'}",
                screen + b"\x1b[" + params + bytes([final]) + b"Z" + query, 12, 4)
    for final in b"Hfr":
        for params in (b"2;3", b"2;3;1", b"2;3;0;1"):
            add(f"csi-count-{final:x}-{params.hex()}",
                screen + b"\x1b[" + params + bytes([final]) + b"Z\x1bP$qr\x1b\\" + query, 12, 4)
    for private in (b"", b"?"):
        for final in b"JK":
            for params in (b"2", b"2;0", b"0;2"):
                add(f"csi-count-{private.hex()}{final:x}-{params.hex()}",
                    screen + b"\x1b[" + private + params + bytes([final]) + b"Z" + query, 12, 4)
    # A pending wrap survives scrolling (RI at the top margin, SU, SD) but
    # not movement.
    for i, prefix in enumerate((b"", b"\r\n", b"\x1b[2;3r\x1b[2;1H", b"\x1b[?69h\x1b[2;5s\x1b[1;2H")):
        for sequence in (b"\x1bM", b"\x1bD", b"\x1bE", b"\x9d\x1b\\", b"\x1b[S", b"\x1b[T", b"\x1b[0S",
                         b"\x1b[0a", b"\x1b[0e", b"\x1b[0I", b"\x1b[0L", b"\x1b[0P", b"\x1b[k", b"\x1b[j"):
            add(f"pending-wrap-{i}-{sequence.hex()}", prefix + b"a" * 10 + sequence + b"X" + query)
    # CTC and DECST8C, from default and cleared tab stops.
    tabs = b"\r\t\x1b[6n\t\x1b[6n\t\x1b[6n"
    for start in (b"", b"\x1b[3g"):
        for sequence in (b"W", b"0W", b"2W", b"5W", b"3W", b"2;5W", b"?5W", b"?W", b"?2W", b"?5;1W"):
            add(f"tabs-{start.hex() or 'default'}-{sequence.hex()}",
                start + b"\x1b[1;9H\x1b[" + sequence + b"\x1b[1;5H\x1b[W" + tabs, 20, 2)
    for intermediates in (b"", b"(", b"((", b"!'", b"!' ", b"!'  ", b"(((((", b"#"):
        for final in (b"0", b"?", b"8", b"B", b"c", b"\\", b"~"):
            add(f"escape-{intermediates.hex() or 'none'}-{final.hex()}",
                b"ab\x1b" + intermediates + final + b"XY" + query)
    points = [cp for first, last in WIDTH_RANGES for cp in range(first, last + 1)]
    points += [0x1100, 0x1F1E6, 0x3400, 0xE9, 0xAD, 0x0301, 0x200D]
    for unicode in (False, True):
        for start in range(0, len(points), 8):
            batch = points[start:start + 8]
            data = (b"\x1b[?2027h" if unicode else b"") + b"".join(
                b"A" + chr(cp).encode() + b"B\r\n" for cp in batch)
            add(f"width-{'unicode' if unicode else 'legacy'}-{batch[0]:x}", data + query, 4, len(batch) + 1)
    rng = random.Random(0x5EED_C1)
    for i in range(200):
        data = b"".join(rng.choice(RAW) if rng.random() < 0.15 else rng.choice(PIECES)
                        for _ in range(rng.randint(1, 40)))
        add(f"seeded-{i}", data + query, 12, 4)
    return cases


if __name__ == "__main__":
    parser = argparse.ArgumentParser(add_help=False)
    parser.add_argument("--compact-fixtures", type=Path)
    args, remaining = parser.parse_known_args()
    sys.argv = [sys.argv[0], *remaining]
    differential.fixtures = fixtures
    differential.main()
    if args.compact_fixtures:
        parser = argparse.ArgumentParser(add_help=False)
        parser.add_argument("--report", type=Path, required=True)
        report_args, _ = parser.parse_known_args(remaining)
        report = json.loads(report_args.report.read_text())
        oracle = differential.Oracle(report["library"])
        fields = ("text", "width", "foreground", "background", "underline_color", "attrs")
        records = []
        for case in fixtures():
            data = case["data"]
            snapshot = oracle.snapshot(data, case["rows"], case["cols"], 65536)
            records.append({
                "name": case["name"], "rows": case["rows"], "cols": case["cols"],
                "input": list(data), "input_sha256": hashlib.sha256(data).hexdigest(),
                "cursor": snapshot["cursor"], "responses": snapshot["responses"],
                "modes": snapshot["modes"],
                "lines": [{"wrapped": row["wrapped"], "cells": [[c[k] for k in fields] for c in row["cells"]]}
                          for row in snapshot["lines"]],
            })
        args.compact_fixtures.parent.mkdir(parents=True, exist_ok=True)
        args.compact_fixtures.write_text(json.dumps({
            "upstream_revision": report["upstream_revision"],
            "library_sha256": report["library_sha256"],
            "generator_sha256": differential.digest(Path(__file__)),
            "cell_fields": list(fields),
            "cases": records,
        }, ensure_ascii=False, separators=(",", ":")) + "\n")
