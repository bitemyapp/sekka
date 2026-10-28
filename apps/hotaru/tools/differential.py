#!/usr/bin/env python3
"""Compare Hotaru replay snapshots against the pinned libghostty-vt oracle.

This is development tooling, never a runtime dependency. ctypes layouts are
checked against the library's own ABI manifest before any terminal is created.
Expected snapshots come exclusively from real calls into that library.
"""
import argparse
import ctypes as C
import datetime
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile


class Rgb(C.Structure):
    _fields_ = [("r", C.c_uint8), ("g", C.c_uint8), ("b", C.c_uint8)]


class ColorValue(C.Union):
    _fields_ = [("palette", C.c_uint8), ("rgb", Rgb), ("_padding", C.c_uint64)]


class Color(C.Structure):
    _fields_ = [("tag", C.c_int), ("value", ColorValue)]


class Style(C.Structure):
    _fields_ = [("size", C.c_size_t), ("fg_color", Color), ("bg_color", Color),
                ("underline_color", Color)] + [(name, C.c_bool) for name in
                ("bold", "italic", "faint", "blink", "inverse", "invisible", "strikethrough", "overline")] + [("underline", C.c_int)]


class Coordinate(C.Structure):
    _fields_ = [("x", C.c_uint16), ("y", C.c_uint32)]


class PointValue(C.Union):
    _fields_ = [("coordinate", Coordinate), ("_padding", C.c_uint64 * 2)]


class Point(C.Structure):
    _fields_ = [("tag", C.c_int), ("value", PointValue)]


class GridRef(C.Structure):
    _fields_ = [("size", C.c_size_t), ("node", C.c_void_p), ("x", C.c_uint16), ("y", C.c_uint16)]


class ScreenExtra(C.Structure):
    _fields_ = [("size", C.c_size_t)] + [(name, C.c_bool) for name in
                ("cursor", "style", "hyperlink", "protection", "kitty_keyboard", "charsets")]


class TerminalExtra(C.Structure):
    _fields_ = [("size", C.c_size_t)] + [(name, C.c_bool) for name in
                ("palette", "modes", "scrolling_region", "tabstops", "pwd", "keyboard")] + [("screen", ScreenExtra)]


class FormatOptions(C.Structure):
    _fields_ = [("size", C.c_size_t), ("emit", C.c_int), ("unwrap", C.c_bool),
                ("trim", C.c_bool), ("extra", TerminalExtra), ("selection", C.c_void_p)]


class ModeConfig(C.Structure):
    _fields_ = [("mode", C.c_uint16), ("value", C.c_bool)]


LAYOUTS = {"GhosttyColorRgb": Rgb, "GhosttyStyleColorValue": ColorValue,
           "GhosttyStyleColor": Color, "GhosttyStyle": Style,
           "GhosttyPointCoordinate": Coordinate, "GhosttyPointValue": PointValue,
           "GhosttyPoint": Point, "GhosttyGridRef": GridRef,
           "GhosttyFormatterScreenExtra": ScreenExtra,
           "GhosttyFormatterTerminalExtra": TerminalExtra,
           "GhosttyFormatterTerminalOptions": FormatOptions,
           "GhosttyTerminalModeConfig": ModeConfig}


def fixture_palette():
    # Explicit shared fixture configuration: xterm palette, independent of
    # Ghostty's application theme. No expected terminal output comes from Rust.
    colors = [[0, 0, 0], [205, 0, 0], [0, 205, 0], [205, 205, 0],
              [0, 0, 238], [205, 0, 205], [0, 205, 205], [229, 229, 229],
              [127, 127, 127], [255, 0, 0], [0, 255, 0], [255, 255, 0],
              [92, 92, 255], [255, 0, 255], [0, 255, 255], [255, 255, 255]]
    levels = [0, 95, 135, 175, 215, 255]
    colors.extend([[levels[i // 36], levels[(i // 6) % 6], levels[i % 6]] for i in range(216)])
    colors.extend([[8 + i * 10] * 3 for i in range(24)])
    return colors


def digest(path):
    with Path(path).open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def checked(result):
    if result != 0:
        raise RuntimeError(f"Ghostty C API returned {result}")


class Oracle:
    def __init__(self, library):
        self.library = Path(library).resolve()
        self.lib = C.CDLL(str(self.library))
        self.lib.ghostty_type_json.argtypes = []
        self.lib.ghostty_type_json.restype = C.c_char_p
        self.manifest = json.loads(self.lib.ghostty_type_json())
        if self.manifest["abi"]["os"] != "linux":
            raise RuntimeError("oracle requires Linux")
        for name, layout in LAYOUTS.items():
            expected = self.manifest["types"][name]
            if C.sizeof(layout) != expected["size"] or C.alignment(layout) != expected["align"]:
                raise RuntimeError(f"ABI size/alignment mismatch for {name}")
            for field, typ in layout._fields_:
                offset = getattr(layout, field).offset
                if field in expected.get("fields", {}):
                    actual = expected["fields"][field]
                    if offset != actual["offset"] or C.sizeof(typ) != actual["size"]:
                        raise RuntimeError(f"ABI field mismatch for {name}.{field}")
        signatures = {
            "ghostty_terminal_new": ([C.c_void_p, C.POINTER(C.c_void_p), C.c_uint16, C.c_uint16], C.c_int),
            "ghostty_terminal_free": ([C.c_void_p], None),
            "ghostty_terminal_vt_write": ([C.c_void_p, C.c_void_p, C.c_size_t], None),
            "ghostty_terminal_get": ([C.c_void_p, C.c_int, C.c_void_p], C.c_int),
            "ghostty_terminal_set": ([C.c_void_p, C.c_int, C.c_void_p], C.c_int),
            "ghostty_terminal_grid_ref": ([C.c_void_p, Point, C.POINTER(GridRef)], C.c_int),
            "ghostty_grid_ref_cell": ([C.POINTER(GridRef), C.POINTER(C.c_uint64)], C.c_int),
            "ghostty_grid_ref_row": ([C.POINTER(GridRef), C.POINTER(C.c_uint64)], C.c_int),
            "ghostty_grid_ref_style": ([C.POINTER(GridRef), C.POINTER(Style)], C.c_int),
            "ghostty_grid_ref_graphemes": ([C.POINTER(GridRef), C.POINTER(C.c_uint32), C.c_size_t, C.POINTER(C.c_size_t)], C.c_int),
            "ghostty_cell_get": ([C.c_uint64, C.c_int, C.c_void_p], C.c_int),
            "ghostty_row_get": ([C.c_uint64, C.c_int, C.c_void_p], C.c_int),
            "ghostty_formatter_terminal_new": ([C.c_void_p, C.POINTER(C.c_void_p), C.c_void_p, FormatOptions], C.c_int),
            "ghostty_formatter_free": ([C.c_void_p], None),
            "ghostty_formatter_format_buf": ([C.c_void_p, C.c_void_p, C.c_size_t, C.POINTER(C.c_size_t)], C.c_int),
        }
        for name, (arguments, result) in signatures.items():
            function = getattr(self.lib, name)
            function.argtypes, function.restype = arguments, result

    def enum(self, name, value):
        return self.manifest["types"][name]["values"][value]

    def query(self, function, obj, enum_name, name, typ):
        output = typ()
        checked(getattr(self.lib, function)(obj, self.enum(enum_name, name), C.byref(output)))
        return output.value if hasattr(output, "value") else output

    def color(self, value):
        if value.tag == self.enum("GhosttyStyleColorTag", "NONE"):
            return {"default": True}
        if value.tag == self.enum("GhosttyStyleColorTag", "PALETTE"):
            return {"index": value.value.palette}
        if value.tag == self.enum("GhosttyStyleColorTag", "RGB"):
            return {"rgb": [value.value.rgb.r, value.value.rgb.g, value.value.rgb.b]}
        raise RuntimeError(f"unknown color tag {value.tag}")

    def formatted(self, terminal):
        options = FormatOptions(size=C.sizeof(FormatOptions), emit=self.enum("GhosttyFormatterFormat", "PLAIN"), trim=True, unwrap=False)
        options.extra.size = C.sizeof(TerminalExtra)
        options.extra.screen.size = C.sizeof(ScreenExtra)
        formatter = C.c_void_p()
        checked(self.lib.ghostty_formatter_terminal_new(None, C.byref(formatter), terminal, options))
        try:
            length = C.c_size_t()
            result = self.lib.ghostty_formatter_format_buf(formatter, None, 0, C.byref(length))
            if result not in (0, -3):
                checked(result)
            buffer = C.create_string_buffer(length.value)
            checked(self.lib.ghostty_formatter_format_buf(formatter, buffer, len(buffer), C.byref(length)))
            return buffer.raw[:length.value].decode("utf-8")
        finally:
            self.lib.ghostty_formatter_free(formatter)

    def snapshot(self, data, rows, cols, chunk):
        terminal = C.c_void_p()
        checked(self.lib.ghostty_terminal_new(None, C.byref(terminal), cols, rows))
        try:
            zero = C.c_size_t(0)
            checked(self.lib.ghostty_terminal_set(terminal, self.enum("GhosttyTerminalOption", "SCROLLBACK_MAX_BYTES"), C.byref(zero)))
            palette = (Rgb * 256)(*(Rgb(*rgb) for rgb in fixture_palette()))
            for name, value in (("COLOR_PALETTE", palette), ("COLOR_FOREGROUND", Rgb(229, 229, 229)), ("COLOR_BACKGROUND", Rgb(0, 0, 0))):
                checked(self.lib.ghostty_terminal_set(terminal, self.enum("GhosttyTerminalOption", name), C.byref(value)))
            replies = bytearray()
            callback_type = C.CFUNCTYPE(None, C.c_void_p, C.c_void_p, C.c_void_p, C.c_size_t)
            callback = callback_type(lambda terminal, userdata, pointer, length: replies.extend(C.string_at(pointer, length)))
            checked(self.lib.ghostty_terminal_set(terminal, self.enum("GhosttyTerminalOption", "WRITE_PTY"), C.cast(callback, C.c_void_p)))
            for start in range(0, len(data), chunk):
                part = data[start:start + chunk]
                self.lib.ghostty_terminal_vt_write(terminal, part, len(part))
            q = lambda name, typ: self.query("ghostty_terminal_get", terminal, "GhosttyTerminalData", name, typ)
            if q("VT_PROCESSING_ERROR", C.c_bool):
                raise RuntimeError("reference terminal processing failed; refusing fixture")
            result = {"rows": rows, "cols": cols, "cursor": {"row": q("CURSOR_Y", C.c_uint16), "col": q("CURSOR_X", C.c_uint16), "visible": q("CURSOR_VISIBLE", C.c_bool)}, "lines": []}
            result["responses"] = list(replies)
            result["modes"] = {"alternate_screen": bool(q("ACTIVE_SCREEN", C.c_int))}
            for name, number in (("application_cursor", 1), ("bracketed_paste", 2004), ("focus_events", 1004)):
                mode = ModeConfig(number, False)
                checked(self.lib.ghostty_terminal_get(terminal, self.enum("GhosttyTerminalData", "MODE"), C.byref(mode)))
                result["modes"][name] = mode.value
            palette = q("COLOR_PALETTE", Rgb * 256)
            result["palette"] = [[rgb.r, rgb.g, rgb.b] for rgb in palette]
            for name, field in (("COLOR_FOREGROUND", "default_foreground"), ("COLOR_BACKGROUND", "default_background")):
                rgb = q(name, Rgb)
                result[field] = [rgb.r, rgb.g, rgb.b]
            extra = {"cursor_pending_wrap": q("CURSOR_PENDING_WRAP", C.c_bool), "active_screen": q("ACTIVE_SCREEN", C.c_int), "plain": self.formatted(terminal), "extended_styles": []}
            for row in range(rows):
                cells = []
                wrapped = False
                for col in range(cols):
                    point = Point(tag=self.enum("GhosttyPointTag", "ACTIVE"))
                    point.value.coordinate = Coordinate(col, row)
                    ref = GridRef(size=C.sizeof(GridRef))
                    checked(self.lib.ghostty_terminal_grid_ref(terminal, point, C.byref(ref)))
                    cell, raw_row, style = C.c_uint64(), C.c_uint64(), Style(size=C.sizeof(Style))
                    checked(self.lib.ghostty_grid_ref_cell(C.byref(ref), C.byref(cell)))
                    checked(self.lib.ghostty_grid_ref_row(C.byref(ref), C.byref(raw_row)))
                    checked(self.lib.ghostty_grid_ref_style(C.byref(ref), C.byref(style)))
                    wrapped = self.query("ghostty_row_get", raw_row, "GhosttyRowData", "WRAP", C.c_bool)
                    wide = self.query("ghostty_cell_get", cell, "GhosttyCellData", "WIDE", C.c_int)
                    count = C.c_size_t()
                    codepoints = (C.c_uint32 * 128)()
                    checked(self.lib.ghostty_grid_ref_graphemes(C.byref(ref), codepoints, len(codepoints), C.byref(count)))
                    text = "".join(chr(cp) for cp in codepoints[:count.value]) or " "
                    background = self.color(style.bg_color)
                    tag = self.query("ghostty_cell_get", cell, "GhosttyCellData", "CONTENT_TAG", C.c_int)
                    if tag == self.enum("GhosttyCellContentTag", "BG_COLOR_PALETTE"):
                        background = {"index": self.query("ghostty_cell_get", cell, "GhosttyCellData", "COLOR_PALETTE", C.c_uint8)}
                    elif tag == self.enum("GhosttyCellContentTag", "BG_COLOR_RGB"):
                        rgb = self.query("ghostty_cell_get", cell, "GhosttyCellData", "COLOR_RGB", Rgb)
                        background = {"rgb": [rgb.r, rgb.g, rgb.b]}
                    attrs = sum((1 << bit) for bit, enabled in enumerate((style.bold, style.faint, style.italic, style.underline != 0, style.blink, style.inverse, style.invisible, style.strikethrough)) if enabled)
                    attrs |= int(style.overline) << 8
                    if style.underline > 1:
                        attrs |= int(style.underline) << 9
                    cells.append({"text": text, "width": {0: 1, 1: 2, 2: 0, 3: 1}[wide], "foreground": self.color(style.fg_color), "background": background, "underline_color": self.color(style.underline_color), "attrs": attrs})
                    if style.overline or style.underline > 1 or wide == 3:
                        extra["extended_styles"].append({"row": row, "col": col, "underline": style.underline, "overline": style.overline, "wide": wide})
                text = "".join(cell["text"] for cell in cells if cell["width"] != 0).rstrip(" ")
                result["lines"].append({"text": text, "wrapped": wrapped, "cells": cells})
            result["_oracle"] = extra
            return result
        finally:
            self.lib.ghostty_terminal_free(terminal)


def fixtures():
    cases = {
        "ascii": b"hello world",
        "cr-bs": b"abcdef\rXY\bZ",
        "tabs": b"a\tb\r\n12\tZ",
        "cursor": b"hello\x1b[2;3HX\x1b[AY\x1b[2DZ\x1b[2BW",
        "cursor-defaults": b"hello\x1b[;H!\x1b[0;0H?",
        "save-restore": b"abc\x1b7\x1b[4;4HX\x1b8Z",
        "erase-line": b"abcdefghijkl\r\x1b[5CX\x1b[K",
        "erase-display": b"one\r\ntwo\r\nthree\x1b[2;2H\x1b[J",
        "erase-characters": b"abcdefghij\x1b[1;3H\x1b[3X",
        "insert-delete-characters": b"abcdefghij\x1b[1;3H\x1b[2@XY\x1b[2P",
        "insert-delete-lines": b"one\r\ntwo\r\nthree\r\nfour\x1b[2;1H\x1b[Lnew\x1b[4;1H\x1b[M",
        "scroll": b"one\r\ntwo\r\nthree\r\nfour\r\nfive\r\nsix",
        "scroll-region": b"top\x1b[5;1Hbottom\x1b[2;4r\x1b[4;1Ha\r\nb\r\nc",
        "origin-mode": b"\x1b[2;4r\x1b[?6h\x1b[1;1HX\x1b[?6lY",
        "wrap-pending": b"abcdefghijkl",
        "wrap": b"abcdefghijklmnop",
        "no-wrap": b"\x1b[?7labcdefghijklmnop",
        "wide": "A雪花B".encode(),
        "wide-wrap": ("abcdefghijk雪花").encode(),
        "combining": "e\u0301 o\u0308 雪\u0301".encode(),
        "emoji": "👩🏽‍💻🇯🇵".encode(),
        "invalid-utf8": b"a\xf0\x28\x8c\x28z\xed\xa0\x80",
        "sgr-basic": b"\x1b[1;3;4;31;44mA\x1b[22;23;24;39;49mB",
        "sgr-index": b"\x1b[38;5;201;48;5;22mA\x1b[0mB",
        "sgr-rgb": b"\x1b[38;2;10;20;30;48;2;40;50;60mA\x1b[0mB",
        "sgr-colon": b"\x1b[38:2::10:20:30;48:2::40:50:60mA\x1b[0mB",
        "sgr-underline-color": b"\x1b[4;58:2::1:2:3mA\x1b[59mB",
        "sgr-erase-background": b"\x1b[41m\x1b[2J\x1b[Habc",
        "alternate": b"primary\x1b[?1049hOTHER\x1b[?1049lX",
        "alternate-active": b"primary\x1b[?1049hOTHER",
        "cursor-hidden": b"abc\x1b[?25l",
        "c1-csi": b"abc\x9b2JZ",
        "osc-title": b"a\x1b]2;title\x07b",
        "ignored-dcs": b"a\x1bP1;2qDATA\x1b\\b",
        "reset": b"\x1b[31mabc\x1bcZ",
        "dec-line-drawing": b"\x1b(0lqqk\r\nx  x\r\nmqqj\x1b(B",
        "charset-shift": b"\x1b)0\x0elqqk\x0f abc",
        "tab-clear": b"\x1b[3gA\tB\r\n\x1b[5G\x1bH\rC\tD",
        "reverse-index": b"one\r\ntwo\x1b[H\x1bMtop",
        "explicit-scroll": b"one\r\ntwo\r\nthree\x1b[2S\x1b[1T",
        "insert-mode": b"abcdef\x1b[1;3H\x1b[4hXY\x1b[4lZ",
        "newline-mode": b"abc\x1b[20h\nX\x1b[20l\nY",
        "erase-with-style": b"\x1b[1;3;4;31;44m\x1b[2J\x1b[Habc",
        "scroll-with-style": b"\x1b[1;31;44mone\r\ntwo\r\nthree\r\nfour\r\nfive\r\nsix",
        "erase-wide-half": "A雪B\x1b[1;3H\x1b[X".encode(),
        "overwrite-wide-tail": "A雪B\x1b[1;3HX".encode(),
        "insert-wide": "A雪BC\x1b[1;3H\x1b[@X".encode(),
        "delete-wide": "A雪BC\x1b[1;3H\x1b[P".encode(),
        "wrap-control-bs": b"abcdefghijkl\bX",
        "wrap-control-tab": b"abcdefghijkl\tX",
        "sgr-all-flags": b"\x1b[1;2;3;4;5;7;8;9mA\x1b[22;23;24;25;27;28;29mB",
        "csi-param-overflow": b"a\x1b[999999999999999999CX",
        "csi-interrupted": b"a\x1b[2;\x18b\x1b[3\x1aC",
        "osc-cancelled": b"a\x1b]2;title\x18b",
        "dcs-escape-abort": b"abc\x1bPqDATA\x1b[2JZ",
        "utf8-overlong": b"a\xe0\x80\x80b\xf0\x80\x80\x80c",
        "utf8-too-large": b"a\xf4\x90\x80\x80z",
        "alternate-47": b"primary\x1b[?47hALT\x1b[?47lX",
        "alternate-repeat": b"a\x1b[?1049hB\x1b[?1049hC\x1b[?1049lD",
        "erase-inverse": b"\x1b[7;31;44m\x1b[2J\x1b[Habc",
        "erase-inverse-default": b"\x1b[7m\x1b[2J\x1b[Habc",
        "protected-selective-erase": b"\x1b[1\"qKEEP\x1b[0\"qremove\x1b[?2J",
        "protected-normal-erase": b"\x1b[1\"qKEEP\x1b[0\"qremove\x1b[2J",
        "origin-save-restore": b"\x1b[2;4r\x1b[?6h\x1b[2;3H\x1b7\x1b[?6l\x1b[HX\x1b8Y",
        "charset-save-restore": b"\x1b(0\x1b7\x1b(Blq\x1b8lq",
        "charset-reset": b"\x1b)0\x0elq\x1bclq",
        "charset-g2-g3": b"\x1b*0\x1bNq\x1b+0\x1bOqz",
        "wrap-save-restore": b"abcdefghijkl\x1b7\rX\x1b8Y",
        "csi-too-many-params": b"\x1b[" + b"1;" * 80 + b"31mX",
        "osc-escape-abort": b"abc\x1b]2;title\x1b[2JZ",
        "dcs-bell": b"abc\x1bPqDATA\x07MORE\x1b\\Z",
        "scroll-region-outside": b"\x1b[2;4r\x1b[5;1Hbottom\nX",
        "sgr-underline-disable": b"\x1b[4mA\x1b[4:0mB",
        "sgr-malformed-rgb": b"\x1b[38;2;999;2;3mA\x1b[0mB",
        "mode-set": b"\x1b[?1;1004;2004hX",
        "mode-reset": b"\x1b[?1;1004;2004h\x1b[?1;1004;2004lX",
        "mode-full-reset": b"\x1b[?1;1004;2004h\x1bcX",
        "dsr": b"abc\x1b[5n\x1b[6n\x1b[?6n",
        "dsr-origin": b"\x1b[2;4r\x1b[?6h\x1b[2;3H\x1b[6n\x1b[?6n",
        "palette-set": b"\x1b]4;1;rgb:11/22/33\x07\x1b[31mX",
        "palette-reset": b"\x1b]4;1;rgb:11/22/33\x07\x1b]104;1\x07X",
        "palette-query": b"\x1b]4;1;?\x07",
        "default-color-set": b"\x1b]10;#112233\x07\x1b]11;rgb:44/55/66\x07X",
        "default-color-query": b"\x1b]10;?\x07\x1b]11;?\x1b\\",
        "default-color-reset": b"\x1b]10;#112233\x07\x1b]11;#445566\x07\x1b]110\x07\x1b]111\x07X",
        "decrqss-sgr": b"\x1b[1;31m\x1bP$qm\x1b\\",
        "decrqss-margins": b"\x1b[2;4r\x1bP$qr\x1b\\",
        "cursor-style-query": b"\x1b[5 q\x1bP$q q\x1b\\",
        "mode-query": b"\x1b[?2004h\x1b[?2004$p\x1b[?1$p",
        "origin-restore-above-margin": b"\x1b[?6h\x1b7\x1b[3;5r\x1b8\x1b[6n",
        "dcs-parameter-overflow": b"\x1bP" + b";" * 32 + b"qPAYLOAD\x1b\\",
        "palette-body-2047": b"\x1b]4;0;?" + b";0;?" * 511 + b"\x07X",
        "palette-body-2048": b"\x1b]4;0;?" + b";0;?" * 511 + b";\x07X",
        "palette-body-2049": b"\x1b]4;0;?" + b";0;?" * 511 + b";;\x07X",
        "palette-invalid-index-stops": b"\x1b]4;0;?;BAD;?;1;?\x07X",
        "palette-invalid-color-stops": b"\x1b]4;0;?;1;INVALID;2;?\x07X",
        "palette-empty-field": b"\x1b]4;0;?;;1;?\x07X",
        "colors-ris-preserves-dynamic": b"\x1b]4;1;#112233\x07\x1b]10;#445566\x07\x1b]11;#778899\x07\x1bc\x1b]4;1;?\x07\x1b]10;?\x07\x1b]11;?\x07",
        "colors-ris-then-explicit-reset": b"\x1b]4;1;#112233\x07\x1b]10;#445566\x07\x1b]11;#778899\x07\x1bc\x1b]104;1\x07\x1b]110\x07\x1b]111\x07\x1b]4;1;?\x07\x1b]10;?\x07\x1b]11;?\x07",
        "device-attributes": b"\x1b[c\x1b[>c\x1b[=c\x1bZ",
        "device-attributes-parameters": b"\x1b[1c\x1b[>2;3c\x1b[=1c\x1b[?1c",
        "mode-query-malformed": b"\x1b[$p\x1b[?$p\x1b[?1;2$p\x1b[?1;$p\x1b[?9999$p",
        "soft-reset-cursor-style": b"\x1b[2;4r\x1b[?6h\x1b[2;3H\x1b[31;4:3;53mA\x1b7\x1b[!pB\x1b8C\x1bP$qm\x1b\\\x1bP$qr\x1b\\",
        "soft-reset-colors": b"\x1b]4;1;#112233\x07\x1b]10;#445566\x07\x1b]11;#778899\x07\x1b[!p\x1b]4;1;?\x07\x1b]10;?\x07\x1b]11;?\x07",
        "soft-reset-modes": b"\x1b[?1;6;66;1004;2004h\x1b[?7;25l\x1b[4;20h\x1b[>31u\x1b[>4;2m\x1b[!p\x1b[?1$p\x1b[?6$p\x1b[?7$p\x1b[?25$p\x1b[?66$p\x1b[4$p\x1b[20$p\x1b[?u",
        "soft-reset-invalid": b"\x1b[31m\x1b[0!pA\x1bP$qm\x1b\\",
        "protection-query-invalid": b"\x1b[1\"q\x1b[3\"qA\x1bP$q\"q\x1b\\",
        "sgr-reset-empty": b"\x1b[31m\x1b[mA\x1bP$qm\x1b\\",
    }
    for index, value in enumerate((
        "4:0", "4:1", "4:2", "4:3", "4:4", "4:5", "4:6", "4:99", "21", "24", "4:", "4:2:3", "4::", "4:2;4",
        "53", "53;55", "1;2;3;4:2;5;7;8;9;53;31;44;58;5;1", "22", "0:1", "1:2", "1;", "1;;", "1;0",
        "38;5;300", "48;5;257", "58;5;999", "38;2;300;400;500", "38:2:1:2:3", "38:2::1:2:3", "38:2:9:1:2:3",
        "38:2:1:2:3:4:5", "38;2;1;2", "38:2:1:2", "38;1;2", "38:5:1:2", "58:2::1:2:3", "58:2:1:2:3;59",
        "38;5;123;48;5;234", "38;5;9;48;5;10", "38;2;12;23;34;48;2;45;56;67",
    )):
        cases[f"sgr-extended-{index:02}"] = b"\x1b[31;4;1m\x1b[" + value.encode() + b"mA\x1bP$qm\x1b\\"
    return [{"name": name, "data": data, "rows": 5, "cols": 12} for name, data in cases.items()]


def differences(expected, actual, path="", limit=20):
    output = []
    if type(expected) is not type(actual):
        return [{"path": path, "expected": expected, "actual": actual}]
    if isinstance(expected, dict):
        for key in sorted(expected.keys() | actual.keys()):
            if key.startswith("_"):
                continue
            if key not in expected or key not in actual:
                output.append({"path": f"{path}/{key}", "expected": expected.get(key), "actual": actual.get(key)})
            else:
                output.extend(differences(expected[key], actual[key], f"{path}/{key}", limit))
            if len(output) >= limit:
                break
    elif isinstance(expected, list):
        if len(expected) != len(actual):
            output.append({"path": path + "/length", "expected": len(expected), "actual": len(actual)})
        for i, (left, right) in enumerate(zip(expected, actual)):
            output.extend(differences(left, right, f"{path}/{i}", limit))
            if len(output) >= limit:
                break
    elif expected != actual:
        output.append({"path": path, "expected": expected, "actual": actual})
    return output[:limit]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--library", required=True, type=Path)
    parser.add_argument("--baseline-metadata", type=Path,
                        default=Path(__file__).resolve().parent.parent / "upstream/baseline-build.json")
    parser.add_argument("--replay", type=Path)
    parser.add_argument("--fixtures", type=Path)
    parser.add_argument("--report", required=True, type=Path)
    parser.add_argument("--chunks", default="1,7,65536")
    args = parser.parse_args()
    chunks = [int(value) for value in args.chunks.split(",")]
    if not chunks or min(chunks) < 1:
        parser.error("chunks must be positive")
    metadata = json.loads(args.baseline_metadata.read_text())
    if metadata["revision"] != "6ba144158577d033ccb6ed4b574e4013957c9161" or not metadata["build_confirmed"]:
        parser.error("baseline must be a confirmed build of the pinned upstream revision")
    if metadata["binaries"]["libghostty-vt"]["sha256"] != digest(args.library):
        parser.error("oracle library hash does not match the confirmed baseline build")
    oracle = Oracle(args.library)
    records = []
    if args.fixtures:
        args.fixtures.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="hotaru-diff-") as directory:
        for case in fixtures():
            data = case.pop("data")
            fixture_hash = hashlib.sha256(data).hexdigest()
            path = Path(directory) / (case["name"] + ".vt")
            path.write_bytes(data)
            canonical = oracle.snapshot(data, case["rows"], case["cols"], 65536)
            if args.fixtures:
                (args.fixtures / path.name).write_bytes(data)
                (args.fixtures / (case["name"] + ".json")).write_text(json.dumps({"upstream_revision": "6ba144158577d033ccb6ed4b574e4013957c9161", "library_sha256": digest(args.library), "input_sha256": fixture_hash, "snapshot": canonical}, ensure_ascii=False, separators=(",", ":")) + "\n")
            for chunk in chunks:
                expected = oracle.snapshot(data, case["rows"], case["cols"], chunk)
                if expected != canonical:
                    raise RuntimeError(f"reference chunk invariance failed: {case['name']} chunk={chunk}")
                record = {**case, "chunk": chunk, "input_sha256": fixture_hash, "reference_chunk_invariant": True}
                if args.replay:
                    command = [str(args.replay.resolve()), "--data", str(path), "--rows", str(case["rows"]), "--cols", str(case["cols"]), "--chunk", str(chunk), "--format", "json"]
                    result = subprocess.run(command, capture_output=True, text=True, timeout=15)
                    if result.returncode:
                        record.update(status="error", stderr=result.stderr[-4096:])
                    else:
                        actual = json.loads(result.stdout)
                        delta = differences(expected, actual)
                        record.update(status="divergent" if delta else "matched", differences=delta)
                else:
                    record["status"] = "reference-only"
                records.append(record)
    report = {"schema": 1, "recorded_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
              "upstream_revision": "6ba144158577d033ccb6ed4b574e4013957c9161", "library": str(args.library.resolve()), "library_sha256": digest(args.library),
              "candidate": str(args.replay.resolve()) if args.replay else None, "candidate_sha256": digest(args.replay) if args.replay else None,
              "fixture_configuration": {"scrollback_bytes": 0, "palette": "xterm fixture_palette()", "foreground": [229, 229, 229], "background": [0, 0, 0]},
              "compared": ["active cell text/graphemes", "width", "symbolic fg/bg/underline color", "style bits including underline variants and overline", "row wrap", "cursor position/visibility", "PTY replies", "application cursor/focus/bracketed-paste modes", "active screen identity", "palette/effective foreground/background"],
              "not_compared": ["empty cell versus explicit space", "wide spacer-head representation", "hyperlinks", "other terminal modes", "pending wrap", "scrollback", "other effects", "graphics", "rendering", "clipboard transfers"],
              "cases": records}
    args.report.parent.mkdir(parents=True, exist_ok=True)
    args.report.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n")
    counts = {status: sum(case["status"] == status for case in records) for status in sorted({case["status"] for case in records})}
    print(json.dumps(counts))
    if counts.get("divergent") or counts.get("error"):
        sys.exit(1)


if __name__ == "__main__":
    main()
