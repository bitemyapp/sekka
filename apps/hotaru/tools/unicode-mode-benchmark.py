#!/usr/bin/env python3
"""Narrow dense-wide diagnostic in separately configured legacy/Unicode modes.

This does not extend the four reviewed acceptance patterns in benchmark.py.
Only one exact generated Han/newline pattern is measured after full grid/effect
agreement. It is not whole-application or all-Unicode performance certification.
"""
import argparse
import datetime
import fcntl
import hashlib
import json
import os
from pathlib import Path
import statistics
import subprocess
from differential import Oracle, differences, digest


def main():
    p=argparse.ArgumentParser(description=__doc__)
    for name in ("library","reference","replay","candidate-metadata","baseline-metadata","output"):
        p.add_argument("--"+name,required=True,type=Path)
    p.add_argument("--lock",type=Path,default=Path.home()/".cache/hotaru-workstation.lock")
    a=p.parse_args()
    metadata=json.loads(a.candidate_metadata.read_text());baseline=json.loads(a.baseline_metadata.read_text())
    assert metadata["binary_sha256"]==digest(a.replay)
    assert baseline["binaries"]["ghostty-reference-replay"]["sha256"]==digest(a.reference)
    assert baseline["binaries"]["libghostty-vt"]["sha256"]==digest(a.library)
    os.sched_setaffinity(0,{min(os.sched_getaffinity(0))})
    pattern=("雪"*36+"\r\n").encode()
    data=pattern*(2*1024*1024//len(pattern))
    path=a.output.with_suffix(".vt");path.write_bytes(data)
    configurations=[]
    with a.lock.open("a") as lock:
        fcntl.flock(lock,fcntl.LOCK_EX)
        oracle=Oracle(a.library)
        for mode in ("legacy","unicode"):
            common=["--data",str(path),"--rows","12","--cols","120","--chunk","65536","--grapheme-width",mode]
            expected=oracle.snapshot(data,12,120,65536,{2027:mode=="unicode"})
            actual=json.loads(subprocess.check_output([a.replay,*common,"--format","json"]))
            delta=differences(expected,actual)
            assert not delta,delta
            samples={"ghostty":[],"hotaru":[]}
            def measure(engine,count):
                binary=a.reference if engine=="ghostty" else a.replay
                value=json.loads(subprocess.check_output([binary,*common,"--bench-json","--iterations",str(count)]))
                assert value["grapheme_width_method"]==mode and value["response_bytes"]==[0]*count
                assert value["timed_scope"]=="feed-only" and value["input_bytes"]==len(data)
                assert (value["rows"],value["cols"],value["scrollback"],value["chunk_bytes"])==(12,120,0,65536)
                assert len(value["samples_ns"])==count
                return value["samples_ns"]
            for engine in samples:measure(engine,3)
            order=[]
            for round in range(4):
                for engine in (list(samples) if round%2==0 else list(reversed(samples))):
                    samples[engine].extend(measure(engine,5));order.append(engine)
            medians={engine:statistics.median(values) for engine,values in samples.items()}
            configurations.append({"grapheme_width_method":mode,"state_matched":True,"samples_ns":samples,
                                   "median_feed_ns":medians,"hotaru_over_ghostty":medians["hotaru"]/medians["ghostty"],"process_order":order})
        assert metadata["binary_sha256"]==digest(a.replay)
        assert baseline["binaries"]["ghostty-reference-replay"]["sha256"]==digest(a.reference)
    report={"scope":__doc__,"recorded_at":datetime.datetime.now(datetime.timezone.utc).isoformat(),
            "candidate_source_sha256":metadata["source_sha256"],"candidate_sha256":digest(a.replay),
            "reference_sha256":digest(a.reference),"library_sha256":digest(a.library),
            "data_bytes":len(data),"data_sha256":hashlib.sha256(data).hexdigest(),"pattern":"36 U+96EA then CR LF",
            "rows":12,"cols":120,"scrollback":0,"chunk_bytes":65536,
            "cpu_affinity":sorted(os.sched_getaffinity(0)),"configurations":configurations}
    a.output.write_text(json.dumps(report,indent=2)+"\n")
    print(json.dumps({value["grapheme_width_method"]:value["hotaru_over_ghostty"] for value in configurations}))


if __name__=="__main__":main()
