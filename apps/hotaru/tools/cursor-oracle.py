#!/usr/bin/env python3
"""Capture bounded cursor-state fixtures from the unchanged pinned Ghostty C API.

The API defaults to steady block; the native application separately selects the
GUI's default blinking block. Capture render-state shape/blink/visibility and
actual DECRQSS/DECRQM reply bytes. No GUI or clock behavior is inferred here.
"""
import argparse
import ctypes as C
import hashlib
import json
from pathlib import Path
from differential import Oracle, checked


class ScrollValue(C.Union):
    _fields_=[("delta",C.c_ssize_t),("padding",C.c_uint64*2)]
class Scroll(C.Structure):
    _fields_=[("tag",C.c_int),("value",ScrollValue)]


def cases():
    seqs = {"initial": b"", **{f"style-{n}": f"\x1b[{n} q".encode() for n in range(9)},
            "omitted": b"\x1b[ q", "multi": b"\x1b[5;2 q", "subparam": b"\x1b[5:2 q",
            "blink-off": b"\x1b[?12l", "blink-on": b"\x1b[?12h", "hidden": b"\x1b[?25l",
            "show": b"\x1b[?25l\x1b[?25h", "soft": b"\x1b[6 q\x1b[!p", "reset": b"\x1b[6 q\x1bc",
            "saved": b"\x1b[5 q\x1b7\x1b[4 q\x1b8", "saved-csi": b"\x1b[5 q\x1b[s\x1b[4 q\x1b[u",
            "mode12-save": b"\x1b[?12h\x1b[?12s\x1b[?12l\x1b[?12r",
            "mode12-unsaved": b"\x1b[?12h\x1b[?12r"}
    for mode in [47,1047,1049]:
        for first in [1,3,5]:
            for second in [2,4,6]:
                for suffix in [b"", b"\x1b[?12h", b"\x1b[?12l"]:
                    seqs[f"alternate-{mode}-{first}-{second}-{suffix.hex()}"] = (
                        f"\x1b[{first} q\x1b[?{mode}h\x1b[{second} q\x1b[?{mode}l".encode()+suffix)
    for shape, blink in [(None,None),*( (s,b) for s in [0,1,2] for b in [False,True])]:
        for name, data in seqs.items():
            yield {"name":name,"default_shape":shape,"default":blink,"input":list(data)}
    for offset in [1,2,4,20]:
        for row in [0,1,3]:
            yield {"name":f"viewport-{offset}-row{row}","default_shape":None,"default":None,"scroll":offset,
                   "input":list(b"row\r\n"*30+f"\x1b[{row+1};1H".encode())}
    for explicit in [0,1,4,6]:
        for suffix in [b"", b"\x1b[ q", b"\x1b[!p", b"\x1bc"]:
            yield {"name":f"defaults-update-{explicit}-{suffix.hex()}","default_shape":0,"default":True,
                   "input":list(f"\x1b[{explicit} q".encode()),"configure_after":[2,False],"after":list(suffix)}


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root",type=Path,default=Path.home()/"work/ghostty-reference-20261028")
    parser.add_argument("--output",type=Path,required=True)
    args=parser.parse_args(); library=args.root/"zig-out/lib/libghostty-vt.so"
    o=Oracle(library); lib=o.lib
    for name,argv in [("new",[C.c_void_p,C.POINTER(C.c_void_p)]),("update",[C.c_void_p,C.c_void_p]),
                      ("get",[C.c_void_p,C.c_int,C.c_void_p]),("free",[C.c_void_p])]:
        f=getattr(lib,"ghostty_render_state_"+name);f.argtypes=argv;f.restype=None if name=="free" else C.c_int
    def configure(t,shape,blink):
        for name,value in [("DEFAULT_CURSOR_STYLE",C.c_int(shape)),("DEFAULT_CURSOR_BLINK",C.c_bool(blink))]:
            checked(lib.ghostty_terminal_set(t,o.enum("GhosttyTerminalOption",name),C.byref(value)))
    lib.ghostty_terminal_scroll_viewport.argtypes=[C.c_void_p,Scroll]
    lib.ghostty_terminal_scroll_viewport.restype=None
    assert C.sizeof(Scroll)==o.manifest["types"]["GhosttyTerminalScrollViewport"]["size"]
    results=[]
    for case in cases():
        t=C.c_void_p();checked(lib.ghostty_terminal_new(None,C.byref(t),10,4))
        try:
            if case["default"] is not None:configure(t,case["default_shape"],case["default"])
            replies=bytearray();cbtype=C.CFUNCTYPE(None,C.c_void_p,C.c_void_p,C.c_void_p,C.c_size_t)
            cb=cbtype(lambda t,u,p,n:replies.extend(C.string_at(p,n)))
            checked(lib.ghostty_terminal_set(t,o.enum("GhosttyTerminalOption","WRITE_PTY"),C.cast(cb,C.c_void_p)))
            data=bytes(case["input"]);lib.ghostty_terminal_vt_write(t,data,len(data))
            if "configure_after" in case:configure(t,*case["configure_after"])
            data=bytes(case.get("after",[]))+b"\x1bP$q q\x1b\\\x1b[?12$p\x1b[?25$p"
            lib.ghostty_terminal_vt_write(t,data,len(data))
            if "scroll" in case:
                value=Scroll(o.enum("GhosttyTerminalScrollViewportTag","DELTA"),ScrollValue(delta=-case["scroll"]))
                lib.ghostty_terminal_scroll_viewport(t,value)
            state=C.c_void_p();checked(lib.ghostty_render_state_new(None,C.byref(state)))
            try:
                checked(lib.ghostty_render_state_update(state,t));case["responses"]=list(replies)
                for key,typ in [("CURSOR_VISUAL_STYLE",C.c_int),("CURSOR_VISIBLE",C.c_bool),
                                ("CURSOR_BLINKING",C.c_bool),("CURSOR_VIEWPORT_HAS_VALUE",C.c_bool)]:
                    v=typ();checked(lib.ghostty_render_state_get(state,o.enum("GhosttyRenderStateData",key),C.byref(v)));case[key]=v.value
                if case["CURSOR_VIEWPORT_HAS_VALUE"]:
                    for key in ["CURSOR_VIEWPORT_X","CURSOR_VIEWPORT_Y"]:
                        v=C.c_uint16();checked(lib.ghostty_render_state_get(state,o.enum("GhosttyRenderStateData",key),C.byref(v)));case[key]=v.value
            finally:lib.ghostty_render_state_free(state)
            results.append(case)
        finally:lib.ghostty_terminal_free(t)
    pin=json.loads((Path(__file__).resolve().parent.parent/"upstream/ghostty.json").read_text())
    metadata={"upstream_revision":pin["revision"],"library_sha256":hashlib.sha256(library.read_bytes()).hexdigest(),
              "scope":"actual library cursor state and replies; GUI default and timer policy tested separately"}
    fields=["  "+json.dumps(k)+": "+json.dumps(v)+"," for k,v in metadata.items()]
    records=["    "+json.dumps(case,separators=(",",":")) for case in results]
    args.output.write_text("{\n"+"\n".join(fields)+'\n  "cases": [\n'+",\n".join(records)+"\n  ]\n}\n")
    print(f"Captured {len(results)} cursor cases")

if __name__=="__main__":main()
