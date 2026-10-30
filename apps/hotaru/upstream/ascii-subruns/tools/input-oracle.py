#!/usr/bin/env python3
"""Generate real key/mouse encodings from the pinned external Ghostty C API.

No generated expected byte is computed by the Rust implementation. Compact row
schemas are documented in tests/input.rs. Large cross-products exercise modifier
combinations, events, flags, geometry and legacy mode interactions.
"""
import argparse
import ctypes as C
import hashlib
import itertools
import json
from pathlib import Path
from differential import Oracle, checked, digest


class MouseSize(C.Structure):
    _fields_ = [("size", C.c_size_t)] + [(name, C.c_uint32) for name in
                ("screen_width", "screen_height", "cell_width", "cell_height", "padding_top", "padding_bottom", "padding_right", "padding_left")]


class Position(C.Structure):
    _fields_ = [("x", C.c_float), ("y", C.c_float)]


def mods(value):
    # Hotaru uses Kitty's wire-order bits; Ghostty's public input ABI differs.
    return (value & 1) | ((value & 2) << 1) | ((value & 4) >> 1) | (value & 8) | ((value & 192) >> 2)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--library", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--baseline-metadata", type=Path, default=Path(__file__).resolve().parent.parent / "upstream/baseline-build.json")
    args = parser.parse_args()
    metadata = json.loads(args.baseline_metadata.read_text())
    if not metadata["build_confirmed"] or metadata["revision"] != "6ba144158577d033ccb6ed4b574e4013957c9161" or digest(args.library) != metadata["binaries"]["libghostty-vt"]["sha256"]:
        parser.error("requires exact confirmed pinned baseline")
    oracle = Oracle(args.library)
    lib = oracle.lib
    for name, typ in (("GhosttyMouseEncoderSize", MouseSize), ("GhosttyMousePosition", Position)):
        manifest = oracle.manifest["types"][name]
        assert C.sizeof(typ) == manifest["size"] and C.alignment(typ) == manifest["align"]
        for field, field_type in typ._fields_:
            assert getattr(typ, field).offset == manifest["fields"][field]["offset"]
            assert C.sizeof(field_type) == manifest["fields"][field]["size"]
    for family in ("key", "mouse"):
        for object_name in ("event", "encoder"):
            getattr(lib, f"ghostty_{family}_{object_name}_new").argtypes = [C.c_void_p, C.POINTER(C.c_void_p)]
            getattr(lib, f"ghostty_{family}_{object_name}_free").argtypes = [C.c_void_p]
            getattr(lib, f"ghostty_{family}_{object_name}_free").restype = None
        getattr(lib, f"ghostty_{family}_encoder_setopt").argtypes = [C.c_void_p, C.c_int, C.c_void_p]
        getattr(lib, f"ghostty_{family}_encoder_setopt").restype = None
        getattr(lib, f"ghostty_{family}_encoder_encode").argtypes = [C.c_void_p, C.c_void_p, C.c_void_p, C.c_size_t, C.POINTER(C.c_size_t)]
        for field, typ in (("action", C.c_int), ("mods", C.c_uint16)):
            getattr(lib, f"ghostty_{family}_event_set_{field}").argtypes = [C.c_void_p, typ]
            getattr(lib, f"ghostty_{family}_event_set_{field}").restype = None
    for field, typ in (("key", C.c_int), ("consumed_mods", C.c_uint16), ("composing", C.c_bool), ("unshifted_codepoint", C.c_uint32)):
        getattr(lib, f"ghostty_key_event_set_{field}").argtypes = [C.c_void_p, typ]
        getattr(lib, f"ghostty_key_event_set_{field}").restype = None
    lib.ghostty_key_event_set_utf8.argtypes = [C.c_void_p, C.c_void_p, C.c_size_t]
    lib.ghostty_key_event_set_utf8.restype = None
    lib.ghostty_mouse_event_set_button.argtypes = [C.c_void_p, C.c_int]
    lib.ghostty_mouse_event_set_button.restype = None
    lib.ghostty_mouse_event_clear_button.argtypes = [C.c_void_p]
    lib.ghostty_mouse_event_clear_button.restype = None
    lib.ghostty_mouse_event_set_position.argtypes = [C.c_void_p, Position]
    lib.ghostty_mouse_event_set_position.restype = None
    lib.ghostty_mouse_encoder_reset.argtypes = [C.c_void_p]
    lib.ghostty_mouse_encoder_reset.restype = None
    handles = []
    for family in ("key", "mouse"):
        for object_name in ("event", "encoder"):
            handle = C.c_void_p()
            checked(getattr(lib, f"ghostty_{family}_{object_name}_new")(None, C.byref(handle)))
            handles.append((family, object_name, handle))
    ke, kk, me, mm = [item[2] for item in handles]
    buffer = C.create_string_buffer(4096)

    def encoded(family, encoder, event):
        written = C.c_size_t()
        checked(getattr(lib, f"ghostty_{family}_encoder_encode")(encoder, event, buffer, len(buffer), C.byref(written)))
        return list(buffer.raw[:written.value])

    keys, mouse, negotiation, seen = [], [], [], set()
    def key_case(key, text="", unshifted=0, modifiers=0, consumed=0, action=1, composing=False, options=0, kitty=0):
        row = (key, text, unshifted, modifiers, consumed, action, composing, options, kitty)
        if row in seen: return
        seen.add(row)
        lib.ghostty_key_event_set_key(ke, oracle.enum("GhosttyKey", key))
        lib.ghostty_key_event_set_action(ke, action)
        lib.ghostty_key_event_set_mods(ke, mods(modifiers))
        lib.ghostty_key_event_set_consumed_mods(ke, mods(consumed))
        lib.ghostty_key_event_set_composing(ke, composing)
        text_bytes = text.encode()
        lib.ghostty_key_event_set_utf8(ke, text_bytes, len(text_bytes))
        lib.ghostty_key_event_set_unshifted_codepoint(ke, unshifted)
        for index, name in enumerate(("CURSOR_KEY_APPLICATION", "KEYPAD_KEY_APPLICATION", "IGNORE_KEYPAD_WITH_NUMLOCK", "ALT_ESC_PREFIX", "MODIFY_OTHER_KEYS_STATE_2", "BACKARROW_KEY_MODE")):
            value = C.c_bool(bool(options & (1 << index)))
            lib.ghostty_key_encoder_setopt(kk, oracle.enum("GhosttyKeyEncoderOption", name), C.byref(value))
        value = C.c_uint8(kitty)
        lib.ghostty_key_encoder_setopt(kk, oracle.enum("GhosttyKeyEncoderOption", "KITTY_FLAGS"), C.byref(value))
        keys.append([*row, encoded("key", kk, ke)])

    functional = "ESCAPE ENTER TAB BACKSPACE INSERT DELETE ARROW_LEFT ARROW_RIGHT ARROW_UP ARROW_DOWN PAGE_UP PAGE_DOWN HOME END CAPS_LOCK SCROLL_LOCK NUM_LOCK PRINT_SCREEN PAUSE HELP CONTEXT_MENU SHIFT_LEFT SHIFT_RIGHT CONTROL_LEFT CONTROL_RIGHT ALT_LEFT ALT_RIGHT META_LEFT META_RIGHT".split()
    functional += [f"F{i}" for i in range(1,26)]
    functional += [f"NUMPAD_{i}" for i in range(10)]
    functional += ["NUMPAD_" + value for value in "DECIMAL DIVIDE MULTIPLY SUBTRACT ADD ENTER EQUAL SEPARATOR LEFT RIGHT UP DOWN PAGE_UP PAGE_DOWN HOME END INSERT DELETE BEGIN".split()]
    try:
        for key, modifiers, kitty, action in itertools.product(functional, range(16), (0,1,2,4,8,16,31), (0,1,2)):
            key_case(key, modifiers=modifiers, kitty=kitty, action=action)
        for key, modifiers, options in itertools.product(functional, range(16), (1,2,6,8,16,32,48)):
            key_case(key, modifiers=modifiers, options=options)
        characters = [(c.upper(), c, ord(c)) for c in "abcdefghijklmnopqrstuvwxyz"]
        characters += [("DIGIT_"+c, c, ord(c)) for c in "0123456789"]
        characters += [(name,text,ord(text)) for name,text in (("SPACE"," "),("SLASH","/"),("BRACKET_LEFT","["),("BRACKET_RIGHT","]"),("MINUS","-"),("EQUAL","="),("BACKSLASH","\\"),("SEMICOLON",";"),("QUOTE","'"),("BACKQUOTE","`"),("COMMA",","),("PERIOD","."))]
        characters += [("A","A",ord('a')),("A","ф",ord('ф')),("C","с",ord('с')),("A","雪",ord('雪')),("A","e\u0301",ord('e')),("UNIDENTIFIED","日本語",0),("ENTER","確定",0),("BACKSPACE","preedit",0),("ESCAPE","確定",0),("DIGIT_2","@",ord('2')),("MINUS","_",ord('-')),("A","",ord('å')),("I","\t",ord('i')),("M","\r",ord('m'))]
        for (key,text,unshifted),modifiers,kitty in itertools.product(characters, (0,1,2,4,5,6,7,8,64,68,128,132), (0,1,3,8,31)):
            key_case(key,text,unshifted,modifiers=modifiers,kitty=kitty,options=8)
            if modifiers & 1: key_case(key,text,unshifted,modifiers=modifiers,consumed=1,kitty=kitty,options=8)
            key_case(key,text,unshifted,modifiers=modifiers,kitty=kitty,action=0,options=8)
            key_case(key,text,unshifted,modifiers=modifiers,kitty=kitty,composing=True,options=8)
            if kitty==0: key_case(key,text,unshifted,modifiers=modifiers,options=24)
        geometries = [(1000,600,10,20,3,7,9,5),(4000,4000,1,1,0,0,0,0)]
        for geometry in geometries:
            size = MouseSize(C.sizeof(MouseSize), *geometry)
            lib.ghostty_mouse_encoder_setopt(mm, oracle.enum("GhosttyMouseEncoderOption", "SIZE"), C.byref(size))
            for tracking, fmt, button, modifiers, action, point in itertools.product(range(5),range(5),range(10),(0,7),(0,1,2),((0,0),(12.5,30.5),(222,223),(-2,-3),(geometry[0]+1,geometry[1]+1))):
                lib.ghostty_mouse_encoder_reset(mm)
                for name, value in (("EVENT",C.c_int(tracking)),("FORMAT",C.c_int(fmt)),("ANY_BUTTON_PRESSED",C.c_bool(button!=0)),("TRACK_LAST_CELL",C.c_bool(True))):
                    lib.ghostty_mouse_encoder_setopt(mm,oracle.enum("GhosttyMouseEncoderOption",name),C.byref(value))
                if button: lib.ghostty_mouse_event_set_button(me,button)
                else: lib.ghostty_mouse_event_clear_button(me)
                lib.ghostty_mouse_event_set_mods(me,mods(modifiers))
                lib.ghostty_mouse_event_set_action(me,action)
                lib.ghostty_mouse_event_set_position(me,Position(*point))
                first = encoded("mouse",mm,me)
                second = encoded("mouse",mm,me)
                mouse.append([tracking,fmt,list(geometry),button,modifiers,action,*point,button!=0,first,second])
        lib.ghostty_key_encoder_setopt_from_terminal.argtypes=[C.c_void_p,C.c_void_p]
        lib.ghostty_key_encoder_setopt_from_terminal.restype=None
        lib.ghostty_mouse_encoder_setopt_from_terminal.argtypes=[C.c_void_p,C.c_void_p]
        lib.ghostty_mouse_encoder_setopt_from_terminal.restype=None
        sequences=[
            b'',b'\x1b[?u',b'\x1b[>1u',b'\x1b[>31u',b'\x1b[>32u',b'\x1b[>1;2u',
            b'\x1b[>1u\x1b[>2u\x1b[<u',b'\x1b[>1u\x1b[<0u',b'\x1b[>1u\x1b[<9999u',
            b'\x1b[=3;1u\x1b[=4;2u\x1b[=1;3u',b'\x1b[>1u\x1b[=4;0u',
            b'\x1b[>1u'+b'\x1b[>2u'*8+b'\x1b[<7u',
            b'\x1b[>1u\x1b[?1049h',b'\x1b[>1u\x1b[?1049h\x1b[>3u\x1b[?1049l',
            b'\x1b[>1u\x1b[?47h\x1b[>3u\x1b[?47l\x1b[?47h',
            b'\x1b[>1u\x1b[?1049h\x1b[>3u\x1b[?1049l\x1b[?1049h',
            b'\x1b[?1;67h',b'\x1b=\x1b[?1035l',b'\x1b[?66h',b'\x1b=\x1b>',
            b'\x1b[?1036l',b'\x1b[>4;2m',b'\x1b[>4;2m\x1b[>4n',b'\x1b[>4;2m\x1b[>m',
            b'\x1b[?9h',b'\x1b[?1000;1005h',b'\x1b[?1002;1006h',b'\x1b[?1003;1015h',b'\x1b[?1003;1016h',
            b'\x1b[?9;1002h\x1b[?9l',b'\x1b[?1005;1006h\x1b[?1005l',
            b'\x1b[?1002;1006h\x1b[>31u\x1bc',
        ]
        query=b'\x1b[?u'+b''.join(f'\x1b[?{mode}$p'.encode() for mode in (1,9,66,67,1000,1002,1003,1005,1006,1007,1015,1016,1035,1036))
        for sequence in sequences:
            terminal=C.c_void_p()
            checked(lib.ghostty_terminal_new(None,C.byref(terminal),80,24))
            try:
                stream=sequence+query
                lib.ghostty_terminal_vt_write(terminal,stream,len(stream))
                lib.ghostty_key_encoder_setopt_from_terminal(kk,terminal)
                key_results=[]
                for key,text,unshifted,modifier in [('ARROW_UP','',0,0),('NUMPAD_0','',0,0),('BACKSPACE','',0,0),('A','a',97,4),('A','a',97,2)]:
                    lib.ghostty_key_event_set_key(ke,oracle.enum('GhosttyKey',key))
                    lib.ghostty_key_event_set_action(ke,1)
                    lib.ghostty_key_event_set_mods(ke,mods(modifier))
                    lib.ghostty_key_event_set_consumed_mods(ke,0)
                    lib.ghostty_key_event_set_composing(ke,False)
                    lib.ghostty_key_event_set_utf8(ke,text.encode(),len(text))
                    lib.ghostty_key_event_set_unshifted_codepoint(ke,unshifted)
                    key_results.append([key,text,unshifted,modifier,encoded('key',kk,ke)])
                lib.ghostty_mouse_encoder_setopt_from_terminal(mm,terminal)
                size=MouseSize(C.sizeof(MouseSize),1000,600,10,20,3,7,9,5)
                lib.ghostty_mouse_encoder_setopt(mm,oracle.enum('GhosttyMouseEncoderOption','SIZE'),C.byref(size))
                lib.ghostty_mouse_event_set_action(me,0)
                lib.ghostty_mouse_event_set_button(me,1)
                lib.ghostty_mouse_event_set_mods(me,0)
                lib.ghostty_mouse_event_set_position(me,Position(35,55))
                negotiation.append([list(stream),oracle.snapshot(stream,24,80,65536)['responses'],key_results,encoded('mouse',mm,me)])
            finally:
                lib.ghostty_terminal_free(terminal)
    finally:
        for family, object_name, handle in reversed(handles):
            getattr(lib,f"ghostty_{family}_{object_name}_free")(handle)
    result = {"schema":1,"upstream_revision":metadata["revision"],"library_sha256":digest(args.library),
              "generator_sha256":digest(Path(__file__)),"key_cases":keys,"mouse_cases":mouse,"negotiation_cases":negotiation}
    args.output.parent.mkdir(parents=True,exist_ok=True)
    args.output.write_text(json.dumps(result,ensure_ascii=False,separators=(",", ":"))+"\n")
    print(json.dumps({"keys":len(keys),"mouse":len(mouse),"negotiation":len(negotiation),"output":str(args.output),"sha256":digest(args.output)}))


if __name__=="__main__": main()
