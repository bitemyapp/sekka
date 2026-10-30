#!/usr/bin/env python3
"""Record configured Unicode terminal behavior from the pinned real C API."""
import argparse
import ctypes as C
import hashlib
import json
from pathlib import Path
from differential import (Oracle, GridRef, Point, Coordinate, ModeConfig,
                          FormatOptions, TerminalExtra, ScreenExtra, checked, digest)


class Selection(C.Structure):
    _fields_ = [("size", C.c_size_t), ("start", GridRef), ("end", GridRef), ("rectangle", C.c_bool)]


def fixtures():
    sequences = {
        "modifier": "👍🏽", "zwj": "👩\u200d💻", "rainbow": "🏳️\u200d🌈",
        "flag": "🇯🇵", "flags-odd": "🇦🇧🇨", "flags-pair": "🇦🇧🇨🇩",
        "vs16": "❤️", "vs15": "☀︎", "vs16-vs15": "❤️︎",
        "vs-invalid": "a️", "vs-after-mark": "❤\u0301️",
        "keycap": "1️⃣", "combining": "e\u0301\u0323",
        "hangul": "\u1100\u1161\u11a8", "indic": "क्\u200dष",
        "prepend": "\u0600a", "spacing-mark": "कः", "unmatched-modifier": "a🏽",
    }
    for cols in (1, 2, 3, 4, 8):
        for name, value in sequences.items():
            for prefix in dict.fromkeys(("", "x" * (cols - 1))):
                for suffix in ("", "!"):
                    label = f"{name}-c{cols}-prefix{len(prefix)}-suffix{len(suffix)}"
                    yield dict(name=label, data=(prefix + value + suffix).encode(), cols=cols, rows=4, unicode=True)
    for name, value in sequences.items():
        yield dict(name="nowrap-"+name, data=b"\x1b[?7lxxx"+value.encode(), cols=4, rows=3, unicode=True)
        yield dict(name="legacy-"+name, data=value.encode(), cols=12, rows=3, unicode=False)
        yield dict(name="toggle-"+name, data=("\x1b[?2027l"+value+"\x1b[?2027h"+value).encode(), cols=12, rows=3, unicode=True)
        for sizes in (((3,4),(8,4)), ((1,4),(8,4)), ((8,2),(8,4))):
            yield dict(name=f"reflow-{name}-{sizes[0][0]}x{sizes[0][1]}",
                       data=("prefix"+value+"after\r\n"+value*3).encode(),
                       cols=8,rows=4,unicode=True,resizes=sizes)
    for enabled in (False, True):
        operations = {"initial": b"", "set": b"\x1b[?2027h", "reset": b"\x1b[?2027l",
                      "decstr": b"\x1b[?2027l\x1b[!p", "ris": b"\x1b[?2027l\x1bc",
                      "save-restore": b"\x1b[?2027l\x1b[?2027s\x1b[?2027h\x1b[?2027r",
                      "restore-unsaved": b"\x1b[?2027h\x1b[?2027r",
                      "save-on-reset-restore": b"\x1b[?2027h\x1b[?2027s\x1b[?2027l\x1b[?2027r"}
        for name, data in operations.items():
            yield dict(name=f"mode-{enabled}-{name}",data=data+b"\x1b[?2027$p"+"👍🏽".encode(),cols=8,rows=3,unicode=enabled)
        for count in (63,64,65,66):
            for base, mark in (("a","\u0301"),("😀","\U0001d185")):
                yield dict(name=f"cap-{enabled}-{ord(base)}-{count}",data=(base+mark*count).encode(),cols=8,rows=3,unicode=enabled)
    for name, data in {
        "style-between": "\x1b[31m❤\x1b[44m️!",
        "width-narrow-overwrite": "☀︎\bZ",
        "width-wide-overwrite": "❤️\bZ",
        "cursor-zero": "abc\r\u0301",
        "blank-base": "abc\x1b[8G\u0301",
        "wrap-then-narrow": "xxx☀︎!",
        "move-then-wide": "xxx❤\x1b[4G️!",
        "links-between": "\x1b]8;id=a;first\x07❤\x1b]8;id=b;second\x07️!",
        "links-closed": "\x1b]8;id=a;first\x07👍\x1b]8;;\x07🏽!",
        "links-open": "👍\x1b]8;id=b;second\x07🏽!",
        "right-edge-style-links": "xxx\x1b[31m\x1b]8;id=a;first\x07❤\x1b[44m\x1b]8;id=b;second\x07️!",
        "alternate": "👍🏽\x1b[?1049h🇯🇵\x1b[?1049l!",
        "insert": "abc\r\x1b[4h❤️!",
        "erase-tail": "❤️\x1b[2G\x1b[X",
        "erase-head": "❤️\r\x1b[X",
        "wrapped-narrow": "xx☀︎!",
        "nowrap-last-narrow": "\x1b[?7lxx☀︎!",
    }.items():
        yield dict(name=name,data=data.encode(),cols=4,rows=4,unicode=True)
    full="☀"+"\u200d☀"*32
    assert len(full)==65
    for name,suffix in (("narrow","︎"),("narrow-then-wide","︎️"),("narrow-then-text","︎!")):
        yield dict(name="cap-width-"+name,data=(full+suffix).encode(),cols=8,rows=3,unicode=True)


def capture(oracle, terminal, rows, cols):
    lib = oracle.lib
    def ref(row, col):
        point = Point(tag=oracle.enum("GhosttyPointTag", "ACTIVE"))
        point.value.coordinate = Coordinate(col,row)
        value = GridRef(size=C.sizeof(GridRef))
        checked(lib.ghostty_terminal_grid_ref(terminal,point,C.byref(value)))
        return value
    mode=ModeConfig(2027,False)
    checked(lib.ghostty_terminal_get(terminal,oracle.enum("GhosttyTerminalData","MODE"),C.byref(mode)))
    result={"grapheme_cluster": mode.value,"links":[],"formats":{}}
    for row in range(rows):
        line=[]
        for col in range(cols):
            cell=ref(row,col); length=C.c_size_t(); buffer=C.create_string_buffer(4096)
            checked(lib.ghostty_grid_ref_hyperlink_uri(C.byref(cell),buffer,len(buffer),C.byref(length)))
            line.append(list(buffer.raw[:length.value]))
        result["links"].append(line)
    selection=Selection(size=C.sizeof(Selection),start=ref(0,0),end=ref(rows-1,cols-1))
    for kind in ("PLAIN","VT"):
        options=FormatOptions(size=C.sizeof(FormatOptions),emit=oracle.enum("GhosttyFormatterFormat",kind),trim=True,unwrap=True,selection=C.cast(C.pointer(selection),C.c_void_p))
        options.extra.size=C.sizeof(TerminalExtra);options.extra.screen.size=C.sizeof(ScreenExtra)
        formatter=C.c_void_p()
        checked(lib.ghostty_formatter_terminal_new(None,C.byref(formatter),terminal,options))
        try:
            length=C.c_size_t()
            code=lib.ghostty_formatter_format_buf(formatter,None,0,C.byref(length))
            if code not in (0,-3):checked(code)
            buffer=C.create_string_buffer(length.value)
            checked(lib.ghostty_formatter_format_buf(formatter,buffer,len(buffer),C.byref(length)))
            result["formats"][kind.lower()]=list(buffer.raw[:length.value])
        finally:lib.ghostty_formatter_free(formatter)
    return result


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--library",required=True,type=Path)
    parser.add_argument("--output",required=True,type=Path)
    args=parser.parse_args()
    oracle=Oracle(args.library)
    oracle.lib.ghostty_grid_ref_hyperlink_uri.argtypes=[C.POINTER(GridRef),C.c_void_p,C.c_size_t,C.POINTER(C.c_size_t)]
    oracle.lib.ghostty_grid_ref_hyperlink_uri.restype=C.c_int
    oracle.lib.ghostty_terminal_resize.argtypes=[C.c_void_p,C.c_uint16,C.c_uint16,C.c_uint32,C.c_uint32]
    oracle.lib.ghostty_terminal_resize.restype=C.c_int
    cases=[];comparisons=0
    for case in fixtures():
        data=case.pop("data");case["input"]=list(data)
        def resize(o,t):
            for cols,rows in case.get("resizes",[]):
                checked(o.lib.ghostty_terminal_resize(t,cols,rows,8,16))
        get=lambda chunk:oracle.snapshot(data,case["rows"],case["cols"],chunk,{2027:case["unicode"]},lambda o,t:capture(o,t,case["rows"],case["cols"]),resize)
        expected=get(max(1,len(data)))
        for chunk in (1,7):
            actual=get(chunk)
            assert actual==expected,case["name"]
            comparisons+=1
        case["snapshot"]=expected;cases.append(case)
    output={"schema":1,"upstream_revision":"6ba144158577d033ccb6ed4b574e4013957c9161","library_sha256":digest(args.library),"reference_chunk_comparisons":comparisons,"cases":cases}
    args.output.write_text(json.dumps(output,ensure_ascii=False,separators=(",",":"))+"\n")
    print(json.dumps({"cases":len(cases),"reference_chunk_comparisons":comparisons,"sha256":digest(args.output)}))


if __name__=="__main__":main()
