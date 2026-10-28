#!/usr/bin/env python3
"""Serial diagnostic for no-link and one long, persistent OSC8 hyperlink.

This narrow microbenchmark does not certify whole-app performance or all OSC8
behavior. Plain grid/effects, every visible URI and exact VT link transitions
must match the pinned real Ghostty library before any workload is timed.
"""
import argparse
import ctypes as C
import datetime
import fcntl
import hashlib
import json
import os
from pathlib import Path
import platform
import statistics
import subprocess

from differential import Oracle, Point, Coordinate, GridRef, FormatOptions, TerminalExtra, ScreenExtra, checked, digest, differences


def reference_links(oracle, data, rows, cols):
    lib = oracle.lib
    lib.ghostty_grid_ref_hyperlink_uri.argtypes = [C.POINTER(GridRef), C.c_void_p, C.c_size_t, C.POINTER(C.c_size_t)]
    lib.ghostty_grid_ref_hyperlink_uri.restype = C.c_int
    terminal = C.c_void_p()
    checked(lib.ghostty_terminal_new(None, C.byref(terminal), cols, rows))
    try:
        zero = C.c_size_t(0)
        checked(lib.ghostty_terminal_set(terminal, oracle.enum("GhosttyTerminalOption", "SCROLLBACK_MAX_BYTES"), C.byref(zero)))
        for offset in range(0, len(data), 65536):
            part = data[offset:offset+65536]
            lib.ghostty_terminal_vt_write(terminal, part, len(part))
        links = []
        for row in range(rows):
            line = []
            for col in range(cols):
                point = Point(tag=oracle.enum("GhosttyPointTag", "ACTIVE"))
                point.value.coordinate = Coordinate(col, row)
                ref = GridRef(size=C.sizeof(GridRef))
                checked(lib.ghostty_terminal_grid_ref(terminal, point, C.byref(ref)))
                buffer = C.create_string_buffer(4096); length = C.c_size_t()
                checked(lib.ghostty_grid_ref_hyperlink_uri(C.byref(ref), buffer, len(buffer), C.byref(length)))
                line.append(list(buffer.raw[:length.value]))
            links.append(line)
        options = FormatOptions(size=C.sizeof(FormatOptions), emit=oracle.enum("GhosttyFormatterFormat", "VT"), trim=True, unwrap=True)
        options.extra.size=C.sizeof(TerminalExtra); options.extra.screen.size=C.sizeof(ScreenExtra)
        formatter = C.c_void_p()
        checked(lib.ghostty_formatter_terminal_new(None, C.byref(formatter), terminal, options))
        try:
            length = C.c_size_t()
            result = lib.ghostty_formatter_format_buf(formatter, None, 0, C.byref(length))
            if result not in (0,-3): checked(result)
            buffer=C.create_string_buffer(length.value)
            checked(lib.ghostty_formatter_format_buf(formatter, buffer, len(buffer), C.byref(length)))
            vt=buffer.raw[:length.value]
        finally: lib.ghostty_formatter_free(formatter)
        return links, vt
    finally: lib.ghostty_terminal_free(terminal)


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    for name in ("library","reference","replay","probe","candidate-metadata","report"):
        parser.add_argument("--"+name,required=True,type=Path)
    parser.add_argument("--lock",type=Path,default=Path.home()/".cache/hotaru-workstation.lock")
    args=parser.parse_args()
    baseline=json.loads((Path(__file__).resolve().parent.parent/"upstream/baseline-build.json").read_text())
    metadata=json.loads(args.candidate_metadata.read_text())
    assert baseline["binaries"]["libghostty-vt"]["sha256"]==digest(args.library)
    assert baseline["binaries"]["ghostty-reference-replay"]["sha256"]==digest(args.reference)
    assert metadata["binary_sha256"]==digest(args.replay) and metadata["build_confirmed"]
    assert metadata["probe_build"]["sha256"]==digest(args.probe)
    tracked=[args.library,args.reference,args.replay,args.probe,args.candidate_metadata,Path(__file__)]
    fingerprints={str(path):digest(path) for path in tracked}
    for name,want in metadata["sources"].items():
        path=Path(metadata["workspace"])/name
        assert digest(path)==want, f"candidate source changed: {name}"
    os.sched_setaffinity(0,{min(os.sched_getaffinity(0))})
    args.report.parent.mkdir(parents=True,exist_ok=True)
    records=[]
    with args.lock.open("a") as lock:
        fcntl.flock(lock,fcntl.LOCK_EX)
        oracle=Oracle(args.library)
        uri=b"https://example.test/"+b"x"*2010
        assert len(uri)==2031
        prefix=b"\x1b]8;id=bench;"+uri+b"\x1b\\"
        suffix=b"\x1b]8;;\x1b\\"
        for label,pattern in (("ascii",b"abcdefghijklmnopqrstuvwxyz0123456789"*2+b"\r\n"),("unicode",("雪"*36+"\r\n").encode())):
            plain=pattern*(2*1024*1024//len(pattern))
            for linked in (False,True):
                name=label+("-long-link" if linked else "-no-link")
                data=prefix+plain+suffix if linked else plain
                path=args.report.parent/(name+".vt"); path.write_bytes(data)
                rows,cols=12,120
                common=["--data",str(path.resolve()),"--rows",str(rows),"--cols",str(cols),"--chunk","65536"]
                expected=oracle.snapshot(data,rows,cols,65536)
                actual=json.loads(subprocess.check_output([str(args.replay),*common,"--format","json"]))
                assert not differences(expected,actual), differences(expected,actual)
                links,vt=reference_links(oracle,data,rows,cols)
                probe=json.loads(subprocess.check_output([str(args.probe),str(path),str(rows),str(cols)]))
                assert probe["links"]==links, name+" URI mismatch"
                # Public app formatter intentionally writes captured default
                # colors; strip these exact known headers for content parity.
                header=b"\x1b]10;rgb:e5/e5/e5\x1b\\\x1b]11;rgb:00/00/00\x1b\\"
                assert bytes(probe["vt"])==header+vt, name+" VT identity/style mismatch"
                commands={"ghostty":[str(args.reference),*common,"--bench-json"],"hotaru":[str(args.replay),*common,"--bench-json"]}
                samples={engine:{key:[] for key in ("samples_ns","setup_ns","teardown_ns")} for engine in commands}
                def measure(engine,count):
                    value=json.loads(subprocess.check_output([*commands[engine],"--iterations",str(count)]))
                    assert value["input_bytes"]==len(data) and value["scrollback"]==0 and value["chunk_bytes"]==65536
                    assert value["rows"]==rows and value["cols"]==cols and value["timed_scope"]=="feed-only"
                    assert len(value["samples_ns"])==count
                    assert value.get("response_bytes",[0]*count)==[0]*count
                    return value
                for engine in commands: measure(engine,3)
                order=[]
                for round in range(4):
                    for engine in (list(commands) if round%2==0 else list(reversed(commands))):
                        result=measure(engine,5);order.append(engine)
                        for key in samples[engine]: samples[engine][key].extend(result[key])
                for sample in samples.values(): sample["median_feed_ns"]=statistics.median(sample["samples_ns"])
                records.append({"name":name,"data_bytes":len(data),"data_sha256":hashlib.sha256(data).hexdigest(),"rows":rows,"cols":cols,"chunk_bytes":65536,"scrollback":0,"linked_uri_bytes":len(uri) if linked else 0,"grid_effects_equal":True,"every_visible_uri_equal":True,"content_vt_equal":True,"warmups":3,"process_order":order,"measurements":samples,"hotaru_over_ghostty":samples["hotaru"]["median_feed_ns"]/samples["ghostty"]["median_feed_ns"]})
        for path,want in fingerprints.items(): assert digest(Path(path))==want, f"binary/tool changed during measurement: {path}"
        for name,want in metadata["sources"].items(): assert digest(Path(metadata["workspace"])/name)==want, f"source changed during measurement: {name}"
        report={"schema":1,"recorded_at":datetime.datetime.now(datetime.timezone.utc).isoformat(),"scope":"diagnostic zero-history feed for four exact persistent-link/no-link ASCII/Unicode streams; not broad performance acceptance","upstream_revision":baseline["revision"],"library_sha256":digest(args.library),"reference_sha256":digest(args.reference),"candidate_sha256":digest(args.replay),"probe_sha256":digest(args.probe),"candidate_source_sha256":metadata["source_sha256"],"candidate_metadata_sha256":digest(args.candidate_metadata),"candidate_metadata":str(args.candidate_metadata),"host":platform.node(),"cpu_affinity":sorted(os.sched_getaffinity(0)),"tool_sha256":digest(Path(__file__)),"results":records}
        args.report.write_text(json.dumps(report,indent=2)+"\n")
    print(json.dumps({r["name"]:r["hotaru_over_ghostty"] for r in records}))


if __name__=="__main__":main()
