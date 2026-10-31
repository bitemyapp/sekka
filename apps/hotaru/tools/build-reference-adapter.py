#!/usr/bin/env python3
"""Build only the development C adapter; retain the exact pinned Zig library."""
import argparse
import datetime
import fcntl
import hashlib
import json
from pathlib import Path
import subprocess


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    for name in ("source","include","library","baseline","output","metadata"):
        parser.add_argument("--"+name,required=True,type=Path)
    parser.add_argument("--lock",type=Path,default=Path.home()/".cache/hotaru-workstation.lock")
    args=parser.parse_args()
    baseline=json.loads(args.baseline.read_text())
    if baseline["binaries"]["libghostty-vt"]["sha256"]!=digest(args.library):
        parser.error("reference library does not match pinned baseline")
    source_hash=digest(args.source)
    command=["cc","-std=c11","-O3","-march=native","-I",str(args.include),str(args.source),str(args.library),
             "-Wl,-rpath,"+str(args.library.parent),"-o",str(args.output)]
    args.output.parent.mkdir(parents=True,exist_ok=True)
    with args.lock.open("a") as lock:
        fcntl.flock(lock,fcntl.LOCK_EX)
        subprocess.run(command,check=True)
        assert digest(args.source)==source_hash
        baseline["recorded_at"]=datetime.datetime.now(datetime.timezone.utc).isoformat()
        baseline["binaries"]["ghostty-reference-replay"]={"path":str(args.output),"sha256":digest(args.output)}
        baseline["adapter"]={"source":str(args.source),"source_sha256":source_hash,"command":command,
                             "compiler":subprocess.check_output(["cc","--version"],text=True).splitlines()[0],
                             "flags":["-std=c11","-O3","-march=native"],
                             "configured_width_methods":["legacy","unicode"]}
        args.metadata.write_text(json.dumps(baseline,indent=2)+"\n")
    print(json.dumps({"metadata":str(args.metadata),"binary_sha256":digest(args.output)}))


if __name__=="__main__":main()
