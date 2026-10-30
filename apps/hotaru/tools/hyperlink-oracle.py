#!/usr/bin/env python3
"""Record OSC 8 cell metadata and copy formats from the pinned external C API."""
import argparse
import ctypes as C
import json
import html
import re
from html.parser import HTMLParser
from pathlib import Path
from differential import Oracle, GridRef, Point, Coordinate, FormatOptions, TerminalExtra, ScreenExtra, checked, digest


class Selection(C.Structure):
    _fields_ = [("size", C.c_size_t), ("start", GridRef), ("end", GridRef), ("rectangle", C.c_bool)]


class SelectionOptions(C.Structure):
    _fields_ = [("size", C.c_size_t), ("emit", C.c_int), ("unwrap", C.c_bool),
                ("trim", C.c_bool), ("selection", C.POINTER(Selection))]


def balanced_html(raw):
    """Repair crossed upstream style/link scopes without changing their meaning.

    Raw reference bytes are retained separately. This independent parser closes
    and reopens inner elements when an outer scope ends, ensuring valid nesting.
    """
    class Balance(HTMLParser):
        def __init__(self):
            super().__init__(convert_charrefs=False)
            self.stack, self.output = [], []
        def handle_starttag(self, tag, attrs):
            text = self.get_starttag_text()
            self.stack.append((tag, text)); self.output.append(text)
        def handle_endtag(self, tag):
            reopen = []
            while self.stack:
                current = self.stack.pop()
                self.output.append(f"</{current[0]}>")
                if current[0] == tag:
                    break
                reopen.append(current)
            for current in reversed(reopen):
                self.stack.append(current); self.output.append(current[1])
        def handle_data(self, data): self.output.append(data)
        def handle_entityref(self, name): self.output.append(f"&{name};")
        def handle_charref(self, name): self.output.append(f"&#{name};")
    parser = Balance(); parser.feed(raw.decode("ascii")); parser.close()
    assert not parser.stack
    return "".join(parser.output).encode("ascii")


def html_semantics(raw):
    """Project independent CSS and hyperlink scopes onto each decoded character."""
    class Semantics(HTMLParser):
        def __init__(self):
            super().__init__(convert_charrefs=True)
            self.styles, self.links, self.characters = [], [], []
        def handle_starttag(self, tag, attrs):
            attrs = dict(attrs)
            if tag == "div": self.styles.append(attrs.get("style"))
            elif tag == "a": self.links.append(attrs.get("href"))
            else: raise AssertionError(tag)
        def handle_endtag(self, tag):
            if tag == "div": self.styles.pop()
            elif tag == "a": self.links.pop()
        def handle_data(self, data):
            self.characters.extend((char, tuple(self.styles), tuple(self.links)) for char in data)
    parser = Semantics(); parser.feed(raw.decode("ascii")); parser.close()
    assert not parser.styles and not parser.links
    return parser.characters


def corrected_href_html(raw):
    """Decode the upstream byte-entity mistake; preserve invalid URI octets."""
    def attribute(match):
        # Decode numeric entities directly: HTML's Windows-1252 remapping of
        # &#128;..&#159; is itself part of the upstream byte-escaping bug.
        uri=re.sub(r"&(?:#([0-9]+)|([a-z]+));",lambda token:chr(int(token[1])) if token[1] else html.unescape(token[0]),match[1].decode("ascii")).encode("latin1")
        # Python's surrogateescape independently retains each undecodable byte.
        semantic="".join(f"%{ord(c)-0xdc00:02X}" if 0xdc80<=ord(c)<=0xdcff else "".join(f"%{b:02X}" for b in c.encode("utf8")) if ord(c)<32 or 127<=ord(c)<=159 else c
                         for c in uri.decode("utf8","surrogateescape"))
        escaped="".join({"&":"&amp;","<":"&lt;",">":"&gt;",'"':"&quot;","'":"&#39;"}.get(c,c if ord(c)<128 else f"&#{ord(c)};") for c in semantic)
        assert html.unescape(escaped)==semantic
        return b'<a href="'+escaped.encode("ascii")+b'">'
    return re.sub(rb'<a href="([^"]*)">',attribute,raw)


def fixtures():
    begin = b"\x1b]8;id=hello;https://example.test/a?x=1&y=2\x1b\\"
    end = b"\x1b]8;;\x1b\\"
    inputs = {
        "basic": begin + b"hello" + end + b" world",
        "implicit": b"\x1b]8;;https://example.test\x07one" + end + b"-" + b"\x1b]8;;https://example.test\x07two",
        "id-reuse": begin + b"one" + end + begin + b"two",
        "id-uri-change": begin + b"one\x1b]8;id=hello;other\x1b\\two",
        "wide": begin + "雪e\u0301".encode() + end + b"x",
        "wide-edge": b"123456789" + begin + "雪".encode() + end,
        "overwrite": begin + b"linked" + end + b"\rnone",
        "overwrite-same": begin + b"linked\rmore",
        "erase": begin + b"linked" + end + b"\x1b[1;3H\x1b[K",
        "insert": begin + b"linked" + end + b"\x1b[1;3H\x1b[2@",
        "delete": begin + b"linked" + end + b"\x1b[1;3H\x1b[2P",
        "wide-half-delete": begin + "雪雪雪".encode() + end + b"\x1b[1;2H\x1b[P",
        "wide-half-erase": begin + "雪雪雪".encode() + end + b"\x1b[1;2H\x1b[X",
        "wide-half-insert": begin + "雪雪雪".encode() + end + b"\x1b[1;2H\x1b[@",
        "scroll": begin + b"one\r\ntwo\r\nthree\r\nfour\r\nfive" + end,
        "reverse-index": begin + b"abc" + end + b"\x1b[H\x1bM",
        "save-restore": begin + b"a\x1b7" + end + b"b\x1b8c",
        "soft-reset": begin + b"a\x1b[!pb",
        "full-reset": begin + b"a\x1bcb",
        "alternate": begin + b"a\x1b[?1049hb\x1b[?1049lc",
        "empty-id-close": begin + b"a\x1b]8;id=;\x1b\\b",
        "nonempty-id-empty-uri": begin + b"a\x1b]8;id=x;\x1b\\b",
        "incomplete-option": b"\x1b]8;broken:id=x;uri\x1b\\x",
        "unknown-option": b"\x1b]8;unknown=yes:id=x;uri\x1b\\x",
        "duplicate-id": b"\x1b]8;id=a:id=b;uri\x1b\\x",
        "empty-option-continue": b"\x1b]8;id=a::unknown=u:id=b;uri\x1b\\x",
        "leading-empty-option-continue": b"\x1b]8;:unknown=u:id=b;uri\x1b\\x",
        "empty-option-missing-equal-stops": b"\x1b]8;id=a::broken:id=b;uri\x1b\\x",
        "empty-final-id-retains-earlier": b"\x1b]8;id=a:id=;uri\x1b\\x",
        "empty-option": b"\x1b]8;:id=x;uri\x1b\\x",
        "raw-uri": b"\x1b]8;;https://example.test/\xff\xc3\xa9\x1b\\x",
        "unicode-uri": b"\x1b]8;;https://example.test/"+"é雪".encode()+b"\x1b\\x",
        "unicode-uri-percent": b"\x1b]8;;https://example.test/%20/"+"é".encode()+b"/%C3%A9\x1b\\x",
        "unicode-uri-control": b"\x1b]8;;https://example.test/\xc2\x80\x1b\\x",
        "invalid-uri-truncated": b"\x1b]8;;https://example.test/%FF/\xc3\x1b\\x",
        "combining-link-change": b"\x1b]8;id=a;first\x1b\\e\x1b]8;id=b;second\x1b\\"+"\u0301".encode()+b"x",
        "html-escapes": b"\x1b]8;id=a;https://example.test/\"'<>&\x1b\\" + "<>&\"'雪".encode() + end,
        "plain-ascii": b"hello\r\nworld",
        "blank-lines": b"\r\n\r\nhello\r\n\r\nworld",
        "blank-holes": b"a\x1b[3Cb",
        "space-trimming": b"  a   \r\n b  ",
        "styled-spaces": b"a\x1b[41m   \x1b[0mb",
        "background-erase": b"a\x1b[42m\x1b[K",
        "background-hole": b"a\x1b[42m\x1b[3X\x1b[3Cb",
        "styles": b"\x1b[1;2;3;4:3;5;7;8;9;53;38;2;1;2;3;48;5;4;58;2;5;6;7mABC\x1b[0mD",
        "style-link-transitions": begin + b"a\x1b[1mb\x1b[31mc\x1b[0md" + end + b"e",
        "link-style-transitions": b"\x1b[1ma" + begin + b"b" + end + b"c\x1b[0md",
        "link-newline": begin + b"a\r\nb" + end,
        "style-link-crossed": b"\x1b[1ma" + begin + b"b\x1b[0mc" + end + b"d",
        "link-style-crossed": begin + b"a\x1b[1mb" + end + b"c\x1b[0md",
        "wrapped-blanks": b"12345678  ab",
    }
    for n in [2046, 2047, 2048, 2049]:
        inputs[f"body-{n}"] = begin + b"a\x1b]8;;" + b"x" * (n-1) + b"\x1b\\b"
    for name, data in inputs.items():
        yield {"name": name, "cols": 10, "rows": 4, "input": list(data)}
        if name in {"wide", "wide-edge", "basic", "id-reuse", "insert", "delete", "styles", "styled-spaces"}:
            for sizes in [[(6, 4)], [(6, 4), (12, 5)], [(1, 4), (10, 4)], [(4, 2), (13, 6)]]:
                yield {"name": f"{name}-resizes{sizes}", "initial_cols": 10, "initial_rows": 4,
                       "cols": sizes[-1][0], "rows": sizes[-1][1], "resizes": sizes, "input": list(data)}
        if name in {"blank-lines", "blank-holes", "space-trimming", "styled-spaces", "background-erase", "background-hole", "wrapped-blanks", "wide-edge", "styles"}:
            for trim, unwrap in [(False, True), (True, False), (False, False)]:
                yield {"name": f"{name}-trim{trim}-unwrap{unwrap}", "cols": 10, "rows": 4, "input": list(data), "trim": trim, "unwrap": unwrap}
        if name in {"wide", "wide-edge", "styled-spaces", "plain-ascii", "style-link-transitions"}:
            for start, end, rectangle in [((0, 1), (0, 4), False), ((0, 2), (1, 3), False), ((0, 2), (1, 3), True), ((0, 3), (1, 2), True)]:
                yield {"name": f"{name}-selection{start}-{end}-rect{rectangle}", "cols": 10, "rows": 4, "input": list(data), "selection": {"start": start, "end": end, "rectangle": rectangle}}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--library", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    oracle = Oracle(args.library)
    expected = json.loads((Path(__file__).resolve().parent.parent / "upstream/baseline-build.json").read_text())
    if digest(args.library) != expected["binaries"]["libghostty-vt"]["sha256"]:
        raise SystemExit("reference library hash does not match pinned build")
    for name, layout in [("GhosttySelection", Selection), ("GhosttyTerminalSelectionFormatOptions", SelectionOptions)]:
        manifest = oracle.manifest["types"][name]
        assert C.sizeof(layout) == manifest["size"] and C.alignment(layout) == manifest["align"]
        for field, typ in layout._fields_:
            assert getattr(layout, field).offset == manifest["fields"][field]["offset"]
            assert C.sizeof(typ) == manifest["fields"][field]["size"]
    lib = oracle.lib
    lib.ghostty_grid_ref_hyperlink_uri.argtypes = [C.POINTER(GridRef), C.c_void_p, C.c_size_t, C.POINTER(C.c_size_t)]
    lib.ghostty_grid_ref_hyperlink_uri.restype = C.c_int
    lib.ghostty_terminal_selection_format_buf.argtypes = [C.c_void_p, SelectionOptions, C.c_void_p, C.c_size_t, C.POINTER(C.c_size_t)]
    lib.ghostty_terminal_selection_format_buf.restype = C.c_int
    lib.ghostty_terminal_resize.argtypes = [C.c_void_p, C.c_uint16, C.c_uint16, C.c_uint32, C.c_uint32]
    lib.ghostty_terminal_resize.restype = C.c_int
    cases = []
    for case in fixtures():
        terminal = C.c_void_p()
        checked(lib.ghostty_terminal_new(None, C.byref(terminal), case.get("initial_cols", case["cols"]), case.get("initial_rows", case["rows"])))
        try:
            data = bytes(case["input"])
            lib.ghostty_terminal_vt_write(terminal, data, len(data))
            for cols, rows in case.get("resizes", []):
                checked(lib.ghostty_terminal_resize(terminal, cols, rows, 8, 16))
            def ref(row, col):
                point = Point(tag=oracle.enum("GhosttyPointTag", "ACTIVE"))
                point.value.coordinate = Coordinate(col, row)
                result = GridRef(size=C.sizeof(GridRef))
                checked(lib.ghostty_terminal_grid_ref(terminal, point, C.byref(result)))
                return result
            links = []
            for row in range(case["rows"]):
                cells = []
                for col in range(case["cols"]):
                    cell = ref(row, col)
                    length = C.c_size_t()
                    buffer = C.create_string_buffer(4096)
                    checked(lib.ghostty_grid_ref_hyperlink_uri(C.byref(cell), buffer, len(buffer), C.byref(length)))
                    cells.append(list(buffer.raw[:length.value]))
                links.append(cells)
            case["links"] = links
            if spec := case.get("selection"):
                a, b = sorted([spec["start"], spec["end"]])
                if spec["rectangle"]:
                    left, right = sorted([a[1], b[1]])
                    a, b = (a[0], left), (b[0], right)
                selection = Selection(size=C.sizeof(Selection), start=ref(*a), end=ref(b[0], b[1]-1), rectangle=spec["rectangle"])
            else:
                selection = Selection(size=C.sizeof(Selection), start=ref(0, 0), end=ref(case["rows"]-1, case["cols"]-1))
            case["formats"] = {}
            for kind in ["PLAIN", "VT", "HTML"]:
                # Test the content formatter without TerminalFormatter's default
                # whole-palette/state prelude. Application-resolved palette and
                # outer default colors require separate verification.
                options = FormatOptions(size=C.sizeof(FormatOptions), emit=oracle.enum("GhosttyFormatterFormat", kind), unwrap=case.get("unwrap", True), trim=case.get("trim", True), selection=C.cast(C.pointer(selection), C.c_void_p))
                options.extra.size = C.sizeof(TerminalExtra)
                options.extra.screen.size = C.sizeof(ScreenExtra)
                formatter = C.c_void_p()
                checked(lib.ghostty_formatter_terminal_new(None, C.byref(formatter), terminal, options))
                try:
                    length = C.c_size_t()
                    result = lib.ghostty_formatter_format_buf(formatter, None, 0, C.byref(length))
                    if result not in (0, -3): checked(result)
                    buffer = C.create_string_buffer(length.value)
                    checked(lib.ghostty_formatter_format_buf(formatter, buffer, len(buffer), C.byref(length)))
                    case["formats"][kind.lower()] = list(buffer.raw[:length.value])
                    if kind == "HTML":
                        raw = buffer.raw[:length.value]
                        repaired = balanced_html(raw)
                        if repaired != raw:
                            assert case["name"] in {
                                "style-link-crossed", "link-style-crossed",
                                "style-link-transitions-selection(0, 1)-(0, 4)-rectFalse",
                                "style-link-transitions-selection(0, 2)-(1, 3)-rectFalse",
                                "style-link-transitions-selection(0, 2)-(1, 3)-rectTrue",
                                "style-link-transitions-selection(0, 3)-(1, 2)-rectTrue",
                            }, case["name"]
                            assert html_semantics(raw) == html_semantics(repaired)
                            case["formats"]["html_balanced"] = list(repaired)
                            case["html_nesting_repair"] = "Crossed style and hyperlink scopes; per-character CSS/link projection verified equal."
                        corrected=corrected_href_html(repaired)
                        if corrected!=repaired:
                            assert case["name"] in {"raw-uri","unicode-uri","unicode-uri-percent","invalid-uri-truncated","unicode-uri-control"},case["name"]
                            case["html_uri_repair"]="Valid UTF8 URI scalars preserved; undecodable octets percent-escaped without altering existing percent escapes."
                        if corrected!=raw:
                            case["formats"]["html_corrected"]=list(corrected)
                finally:
                    lib.ghostty_formatter_free(formatter)
            cases.append(case)
        finally:
            lib.ghostty_terminal_free(terminal)
    output = {"schema": 1, "upstream_revision": expected["revision"], "library_sha256": digest(args.library), "cases": cases}
    args.output.write_text(json.dumps(output, ensure_ascii=False, separators=(",", ":")) + "\n")
    print(json.dumps({"cases": len(cases), "output": str(args.output)}))


if __name__ == "__main__":
    main()
