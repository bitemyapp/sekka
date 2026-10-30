#!/usr/bin/env python3
"""Record real pinned Ghostty search selections, without consulting Hotaru.

This development-only oracle validates every ctypes layout against Ghostty's
own ABI manifest. Coordinates are whole-screen rows (history then active), and
inclusive endpoints are retained verbatim rather than guessed from text widths.
"""
import argparse
import ctypes as C
import json
import random
from pathlib import Path

from differential import Oracle, Coordinate, GridRef, checked, digest


class String(C.Structure):
    _fields_ = [("ptr", C.c_void_p), ("len", C.c_size_t)]


class Selection(C.Structure):
    _fields_ = [("size", C.c_size_t), ("start", GridRef),
                ("end", GridRef), ("rectangle", C.c_bool)]


class SelectionBuffer(C.Structure):
    _fields_ = [("ptr", C.POINTER(Selection)), ("cap", C.c_size_t), ("len", C.c_size_t)]


def fixtures():
    samples = [
        ("overlap", "aaaa", ["aa", "a", "aaaa", "aaaaa", "", "\n"]),
        ("ascii-case", "AbA aBA", ["aba", "A", "ba a"]),
        ("unicode-case", "É é Éé", ["É", "é", "Éé", "éé"]),
        ("soft-wrap", "abcDEFGHIjkl", ["cde", "fghij", "abcdefghijkl", "\n"]),
        ("hard-newline", "abcd\r\nEF", ["de", "d\nE", "\n", "EF\n", "\n\n"]),
        ("empty", "", [" ", "\n", "\n\n", ""]),
        ("trailing-lines", "a\r\n\r\n", ["a\n", "\n", "\n\n", "a\n\n"]),
        ("blank-interior", "a\r\n\r\nb", ["a\n\nb", "\n", "\nb", "\n\n"]),
        ("spaces", "a  \r\nb c  ", ["a ", "a\nb", " ", "c\n", "c  "]),
        ("spaces-soft", "ab  c", ["b  c", "b c", "  ", "\n"]),
        ("cursor-blanks", "a\x1b[2;4Hb", ["a\n   b", "   ", "\nb", "b\n"]),
        ("wide", "a雪b界", ["雪", "雪b", "b界", "a雪b界", "\n"]),
        ("wide-edge", "abc雪z", ["c雪", "雪", "abc雪z", "c 雪"]),
        ("combining", "xe\u0301Y", ["e", "\u0301", "e\u0301", "\u0301Y", "xe\u0301Y"]),
        ("emoji", "a👩\u200d💻b🇯🇵c👍🏽d", ["👩", "\u200d", "👩\u200d💻", "🇯🇵", "🏽", "b🇯🇵c"]),
        ("history", "old\r\n1\r\n2\r\n3", ["old", "1\n2", "\n", "3\n"]),
        ("alternate", "primary\x1b[?1049halternate", ["primary", "alternate", "\n"]),
        ("primary-return", "primary\x1b[?1049halternate\x1b[?1049l", ["primary", "alternate"]),
        ("erase", "abcdef\x1b[H\x1b[Kxy", ["xy", "abcdef", "\n"]),
        ("blank-wrap", "a   \r\n    b", ["a\nb", "a\n    b", "    ", "\n"]),
    ]
    for name, data, needles in samples:
        for needle in needles:
            yield {"name": f"{name}-{needle!r}", "cols": 4, "rows": 3,
                   "input": list(data.encode()), "needle": list(needle.encode())}
    for sizes in [[(3, 2)], [(12, 6)], [(3, 2), (9, 4)]]:
        for needle in ["OLD", "e\u0301雪", "second\nlast", "\n"]:
            yield {"name": f"reflow-{sizes}-{needle!r}", "cols": 8, "rows": 3,
                   "input": list("old e\u0301雪\r\nsecond\r\nlast".encode()),
                   "resizes": sizes, "needle": list(needle.encode())}
    # More than one upstream page, including a needle across its boundary.
    for needle in ["row0010", "needle\nrow", "needle", "\n"]:
        data = "\r\n".join(f"row{i:04} needle" for i in range(3000))
        yield {"name": f"pages-{needle!r}", "cols": 20, "rows": 4,
               "input": list(data.encode()), "needle": list(needle.encode())}
    rng = random.Random(0x5EA2C4)
    for index in range(30):
        cols, rows = rng.randrange(4, 13), rng.randrange(3, 7)
        data = "".join(rng.choice(["a", "b", "α", "雪", " ", "\r\n", "\r",
                                   "\x1b[K", "\x1b[2J", "\x1b[H", "\x1b[2C",
                                   "\x1b[31m", "\x1b[0m", "e\u0301"])
                       for _ in range(80)).encode()
        for needle in [b"a", b"b", "α".encode(), "雪".encode(), b"a\nb"]:
            yield {"name": f"seeded-{index}-{needle!r}", "cols": cols, "rows": rows,
                   "input": list(data), "needle": list(needle)}


def open_oracle(library):
    expected = json.loads((Path(__file__).resolve().parent.parent / "upstream/baseline-build.json").read_text())
    if digest(library) != expected["binaries"]["libghostty-vt"]["sha256"]:
        raise SystemExit("reference library hash does not match pinned build")
    oracle = Oracle(library)
    for name, layout in [("GhosttyString", String), ("GhosttySelection", Selection),
                         ("GhosttySelectionBuffer", SelectionBuffer)]:
        manifest = oracle.manifest["types"][name]
        assert C.sizeof(layout) == manifest["size"] and C.alignment(layout) == manifest["align"], name
        for field, typ in layout._fields_:
            assert getattr(layout, field).offset == manifest["fields"][field]["offset"], (name, field)
            assert C.sizeof(typ) == manifest["fields"][field]["size"], (name, field)
    lib = oracle.lib
    for name, arguments, result in [
        ("ghostty_search_new", [C.c_void_p, C.POINTER(C.c_void_p), C.c_void_p], C.c_int),
        ("ghostty_search_free", [C.c_void_p], None),
        ("ghostty_search_run", [C.c_void_p], C.c_int),
        ("ghostty_search_set", [C.c_void_p, C.c_int, C.c_void_p], C.c_int),
        ("ghostty_search_get", [C.c_void_p, C.c_int, C.c_void_p], C.c_int),
        ("ghostty_terminal_point_from_grid_ref", [C.c_void_p, C.POINTER(GridRef), C.c_int, C.POINTER(Coordinate)], C.c_int),
        ("ghostty_terminal_resize", [C.c_void_p, C.c_uint16, C.c_uint16, C.c_uint32, C.c_uint32], C.c_int),
    ]:
        function = getattr(lib, name)
        function.argtypes, function.restype = arguments, result
    return oracle, expected


def selections(oracle, search, terminal, exclusive=False):
    lib = oracle.lib
    buffer = SelectionBuffer()
    result = lib.ghostty_search_get(search, oracle.enum("GhosttySearchData", "MATCHES"), C.byref(buffer))
    if result not in (0, -3):
        checked(result)
    raw = (Selection * buffer.len)()
    for selection in raw:
        selection.size = C.sizeof(Selection)
        selection.start.size = selection.end.size = C.sizeof(GridRef)
    buffer.ptr, buffer.cap = raw, buffer.len
    checked(lib.ghostty_search_get(search, oracle.enum("GhosttySearchData", "MATCHES"), C.byref(buffer)))
    matches = []
    for selection in raw:
        points = []
        for ref in [selection.start, selection.end]:
            point = Coordinate()
            checked(lib.ghostty_terminal_point_from_grid_ref(terminal, C.byref(ref), oracle.enum("GhosttyPointTag", "SCREEN"), C.byref(point)))
            points.append([point.y, point.x])
        if exclusive:
            cell = C.c_uint64()
            checked(lib.ghostty_grid_ref_cell(C.byref(selection.end), C.byref(cell)))
            wide = oracle.query("ghostty_cell_get", cell, "GhosttyCellData", "WIDE", C.c_int)
            points[1][1] += 2 if wide == 1 else 1
        assert not selection.rectangle
        matches.append(points)
    return matches


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--library", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    oracle, expected = open_oracle(args.library)
    lib = oracle.lib
    cases = []
    for case in fixtures():
        terminal, search = C.c_void_p(), C.c_void_p()
        checked(lib.ghostty_terminal_new(None, C.byref(terminal), case["cols"], case["rows"]))
        try:
            # libghostty-vt defaults to 10,000 bytes, unlike the application's
            # 50 MB setting. Configure this explicitly before feeding history.
            history_bytes = C.c_size_t(50_000_000)
            checked(lib.ghostty_terminal_set(terminal, oracle.enum("GhosttyTerminalOption", "SCROLLBACK_MAX_BYTES"), C.byref(history_bytes)))
            data = bytes(case["input"])
            lib.ghostty_terminal_vt_write(terminal, data, len(data))
            for cols, rows in case.get("resizes", []):
                checked(lib.ghostty_terminal_resize(terminal, cols, rows, 8, 16))
            checked(lib.ghostty_search_new(None, C.byref(search), terminal))
            needle_bytes = C.create_string_buffer(bytes(case["needle"]))
            needle = String(C.cast(needle_bytes, C.c_void_p), len(case["needle"]))
            checked(lib.ghostty_search_set(search, oracle.enum("GhosttySearchOption", "NEEDLE"), C.byref(needle)))
            checked(lib.ghostty_search_run(search))
            case["matches"] = selections(oracle, search, terminal)
            total = oracle.query("ghostty_search_get", search, "GhosttySearchData", "TOTAL_MATCHES", C.c_size_t)
            assert total == len(case["matches"])
            # Upstream appendBlankPoints emits backwards coordinates for a
            # forward run of spaces. Preserve raw evidence and name each
            # independently checked correction, never normalize all matches.
            corrections = {
                "spaces-soft-'  '": ([[[0, 3], [0, 2]]], [[[0, 2], [0, 3]]]),
                "cursor-blanks-'   '": ([[[1, 2], [1, 0]]], [[[1, 0], [1, 2]]]),
                "blank-wrap-'    '": ([[[1, 3], [1, 0]]], [[[1, 0], [1, 3]]]),
            }
            if case["name"] in corrections:
                raw, corrected = corrections[case["name"]]
                assert case["matches"] == raw
                case["corrected_matches"] = corrected
                case["correction"] = "Forward whitespace cells retain forward coordinates; upstream appendBlankPoints reverses this run."
            cases.append(case)
        finally:
            if search.value:
                lib.ghostty_search_free(search)
            lib.ghostty_terminal_free(terminal)
    args.output.write_text(json.dumps({"schema": 1, "upstream_revision": expected["revision"], "reference_history_limit_bytes": 50_000_000,
                                      "library_sha256": digest(args.library), "cases": cases},
                                     ensure_ascii=False, separators=(",", ":")) + "\n")
    print(f"Recorded {len(cases)} search cases from unchanged Ghostty.")


if __name__ == "__main__":
    main()
