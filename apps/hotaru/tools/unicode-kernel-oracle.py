#!/usr/bin/env python3
"""Exhaustive scalar widths and reproducible grapheme probes against real C API."""
import argparse
import ctypes as C
import hashlib
import itertools
import json
from pathlib import Path
import random
import re
import struct
import subprocess


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("library", "probe", "table", "ucd", "report"):
        parser.add_argument("--" + name, required=True, type=Path)
    args = parser.parse_args()
    lib = C.CDLL(str(args.library))
    width = lib.ghostty_unicode_codepoint_width
    width.argtypes, width.restype = [C.c_uint32], C.c_uint8
    cluster = lib.ghostty_unicode_grapheme_width
    cluster.argtypes, cluster.restype = [C.POINTER(C.c_uint32), C.c_size_t, C.POINTER(C.c_uint8)], C.c_size_t
    actual = subprocess.check_output([args.probe, "widths"])
    assert len(actual) == 0x110000
    scalars = 0
    for cp, value in enumerate(actual):
        if 0xd800 <= cp <= 0xdfff:
            assert value == 255
            continue
        expected = width(cp)
        assert value == expected, (hex(cp), value, expected)
        scalars += 1
    data = args.table.read_text()
    stages = []
    for name in ("STAGE1", "STAGE2", "PROPERTIES"):
        text = re.search(rf"static {name}: \[u16; \d+\] = \[(.*?)\];", data, re.S).group(1)
        stages.append([int(value) for value in text.split(",") if value.strip()])
    s1, s2, props = stages
    representatives = {}
    for cp in range(0x100, 0x110000):
        if 0xd800 <= cp <= 0xdfff:
            continue
        value = props[s2[s1[cp >> 8] + (cp & 255)]]
        representatives.setdefault(value, cp)
    symbols = list(representatives.values()) + [0x20, 0x61, 0xfe0e, 0xfe0f]
    cases = [[], *([cp] for cp in symbols)]
    cases.extend(map(list, itertools.product(symbols, repeat=3)))
    ucd_count = 0
    for line in args.ucd.read_text().splitlines():
        body = line.split("#")[0].strip()
        if body:
            chars = [int(value, 16) for value in body.split() if value not in ("÷", "×")]
            if all(chr(cp).isprintable() or cp >= 0x100 for cp in chars):
                cases.append(chars)
                ucd_count += 1
    rng = random.Random(0x202718)
    for _ in range(20000):
        cases.append(rng.choices(symbols, k=rng.randrange(4, 24)))
    request = bytearray()
    expected = bytearray()
    for case in cases:
        request.extend(struct.pack("<H", len(case)))
        request.extend(struct.pack("<" + "I" * len(case), *case))
        result = C.c_uint8()
        consumed = cluster((C.c_uint32 * len(case))(*case), len(case), C.byref(result))
        expected.extend(struct.pack("<HB", consumed, result.value))
    actual = subprocess.check_output([args.probe], input=request)
    assert len(actual) == len(expected)
    differences = []
    for index, case in enumerate(cases):
        offset = index * 3
        if actual[offset:offset + 3] != expected[offset:offset + 3]:
            differences.append({"codepoints": [f"U+{cp:04X}" for cp in case],
                                "actual": struct.unpack("<HB", actual[offset:offset + 3]),
                                "reference": struct.unpack("<HB", expected[offset:offset + 3])})
            if len(differences) >= 20:
                break
    report = {"reference_library_sha256": sha(args.library), "probe_sha256": sha(args.probe),
              "kernel_source_sha256": sha(args.probe.parent / "unicode.rs"),
              "probe_source_sha256": sha(args.probe.parent / "probe.rs"),
              "table_sha256": sha(args.table), "unicode_scalar_width_comparisons": scalars,
              "grapheme_comparisons": len(cases), "ucd_vectors": ucd_count,
              "property_representatives": len(representatives), "seed": "0x202718",
              "request_sha256": hashlib.sha256(request).hexdigest(),
              "response_sha256": hashlib.sha256(expected).hexdigest(), "differences": differences}
    args.report.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report))
    assert not differences


if __name__ == "__main__":
    main()
