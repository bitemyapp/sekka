#!/usr/bin/env python3
"""Pinned-library REP, DEC save/restore and fixed-reset mode comparisons.

Uses differential.py's ABI checks, complete common grid and reply comparison,
source/library identities and chunk-invariance gate. Expected data comes only
from the actual pinned library. The same command-line options are accepted.
"""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import random
import sys

import differential


MODES = (1, 6, 7, 9, 25, 47, 66, 67, 1000, 1002, 1003, 1004, 1005, 1006,
         1007, 1015, 1016, 1035, 1036, 1047, 1048, 1049, 2004, 2027)


def dec(mode, final):
    return f"\x1b[?{mode}{final}".encode()


def fixtures():
    cases = []

    def add(name, data, cols=12, rows=5):
        cases.append(dict(name=name, data=data, cols=cols, rows=rows))

    query = b"".join(dec(mode, "$p") for mode in MODES) + b"\x1b[6n"
    for mode in MODES:
        for name, actions in (
            ("unsaved", "hr"), ("set", "hslr"), ("reset", "lshr"),
            ("overwrite", "hslsr"), ("repeat-restore", "hslrr"),
            ("soft-reset", "hs"), ("full-reset", "hs"),
        ):
            data = b"start\x1b[2;3H" + b"".join(dec(mode, action) for action in actions)
            if name == "soft-reset":
                data += b"\x1b[!p" + dec(mode, "r")
            if name == "full-reset":
                data += b"\x1bc" + dec(mode, "r")
            add(f"mode-{mode}-{name}", data + query + b"Z")
    add("modes-list", b"\x1b[?1;7;25;66;1004;2004h\x1b[?1;7;25;66;1004;2004s"
        b"\x1b[?1;7;25;66;1004;2004l\x1b[?1;7;25;66;1004;2004r" + query)
    add("modes-unknown", b"\x1b[?0;65535;32769;9999s\x1b[?0;65535;32769;9999r" + query)
    add("modes-empty", b"\x1b[?s\x1b[?r" + query)
    add("modes-colon", b"\x1b[?1h\x1b[?1:7s\x1b[?1l\x1b[?1:7r" + query)
    add("mode-origin-restore-homes", b"\x1b[2;4r\x1b[?6h\x1b[?6s\x1b[2;4H"
        b"\x1b[?6r" + query + b"X")
    add("mode-visible-cursor-restore", b"\x1b7\x1b[?25l\x1b8" + query)
    add("mode-hidden-cursor-restore", b"\x1b[?25l\x1b7\x1b[?25h\x1b8" + query)
    for ansi in (False, True):
        modes = (1, 5, 7, 10, 11, 13, 14, 15, 16, 17, 18, 19) if ansi else (117,)
        prefix = "" if ansi else "?"
        for mode in modes:
            data = b"".join(f"\x1b[{prefix}{mode}{action}".encode()
                            for action in ("$p", "h", "$p", "l", "$p"))
            add(f"fixed-mode-{'ansi' if ansi else 'dec'}-{mode}", data)
    repeats = (b"\x1b[b", b"\x1b[0b", b"\x1b[1b", b"\x1b[17b", b"\x1b[65535b",
               b"\x1b[2;3b", b"\x1b[2:b", b"\x1b[2 b", b"\x1b[?2b")
    texts = ("", "A", "abc", "猫", "e\u0301", "☀️", "👩🏽‍💻", "🇯🇵", "\u0301")
    for unicode in (False, True):
        for i, text in enumerate(texts):
            for j, repeat in enumerate(repeats):
                add(f"repeat-{'unicode' if unicode else 'legacy'}-{i}-{j}",
                    (b"\x1b[?2027h" if unicode else b"") + text.encode() + repeat + b"\x1b[6n")
    for i, prefix in enumerate((
        b"A\x1b[31m", b"A\x1b[2J", b"A\x1b7\x1b8", b"A\x1b[!p", b"A\x1bc",
        b"A\x1b[?1049h", b"A\x1b[?7l\x1b[12G", b"A\x1b[4h\x1b[H",
        b"\x1b(0q\x1b(B", b"\x1b(A#\x1b(B", b"\x1b*0\x1bNq", b"\x1b(0q",
        b"abcdefghijkl", b"A\x1b]8;;https://example.org\x1b\\", b"\x1b[1\"qA",
    )):
        add(f"repeat-state-{i}", prefix + b"\x1b[13b\x1b[6n")
    for mode in (47, 1047, 1049):
        for name, body in (
            ("repeat-enter", b"A" + dec(mode, "h") + b"B" + dec(mode, "h") + b"C"),
            ("repeat-exit", b"A" + dec(mode, "l") + b"B" + dec(mode, "l") + b"C"),
            ("clear-timing", b"A" + dec(mode, "h") + b"B" + dec(mode, "l") + dec(47, "h")),
            ("pending-wrap", b"abcdefghijkl" + dec(mode, "h") + b"X" + dec(mode, "l") + b"Y"),
            ("margins", b"\x1b[2;4r" + dec(mode, "h") + b"\x1bP$qr\x1b\\\x1b[3;5r" + dec(mode, "l") + b"\x1bP$qr\x1b\\"),
            ("save-per-screen", b"A\x1b7" + dec(mode, "h") + b"BC\x1b7" + dec(mode, "l") + b"\x1b8Z" + dec(47, "h") + b"\x1b8Q"),
        ):
            add(f"alternate-{mode}-{name}", body + query)
    # Repeated changes exercise independent saved slots and request ordering.
    rng = random.Random(0xDEC5A9E)
    for i in range(32):
        data = bytearray()
        for _ in range(60):
            mode = rng.choice(MODES)
            data += dec(mode, rng.choice("hlsr"))
            data += b"Z" if rng.randrange(5) == 0 else b""
            data += dec(mode, "$p")
        add(f"mode-seeded-{i}", bytes(data) + query)
    # Keep independent adversarial coverage separate and import its generator;
    # the compact Cargo fixture contains both sets of observed expectations.
    review_path=Path(__file__).with_name("vt-controls-review.py")
    spec=importlib.util.spec_from_file_location("hotaru_vt_controls_review",review_path)
    review=importlib.util.module_from_spec(spec)
    spec.loader.exec_module(review)
    cases.extend(review.fixtures())
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
        records = []
        for case in fixtures():
            data = case["data"]
            snapshot = oracle.snapshot(data, case["rows"], case["cols"], 65536)
            records.append({
                "name": case["name"], "rows": case["rows"], "cols": case["cols"],
                "input": list(data), "input_sha256": hashlib.sha256(data).hexdigest(),
                "cursor": snapshot["cursor"], "responses": snapshot["responses"],
                "modes": snapshot["modes"],
                "lines": [{"wrapped": row["wrapped"], "cells": [
                    [c[k] for k in ("text", "width", "foreground", "background", "underline_color", "attrs")]
                    for c in row["cells"]]} for row in snapshot["lines"]],
            })
        args.compact_fixtures.parent.mkdir(parents=True, exist_ok=True)
        args.compact_fixtures.write_text(json.dumps({
            "upstream_revision": report["upstream_revision"],
            "library_sha256": report["library_sha256"],
            "generator_sha256": differential.digest(Path(__file__)),
            "review_generator_sha256": differential.digest(Path(__file__).with_name("vt-controls-review.py")),
            "cell_fields": ["text", "width", "foreground", "background", "underline_color", "attrs"],
            "cases": records,
        }, ensure_ascii=False, separators=(",", ":")) + "\n")
