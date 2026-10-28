#!/usr/bin/env python3
"""Run from an empty result directory; argument is the Perfetto v56.1 SDK dir."""
import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import time

source = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("probes", source / "run-probes.py")
probes = importlib.util.module_from_spec(spec)
spec.loader.exec_module(probes)
sdk = Path(sys.argv[1]).resolve()
exe = Path.cwd() / "perfetto-probe"
build = ["g++", "-std=c++17", "-O2", "-pthread", f"-I{sdk}",
         str(source / "perfetto-probe.cpp"), str(sdk / "perfetto.cc"), "-ldl", "-o", str(exe)]
probes.run(build)
result = {"scope": "synthetic in-process SDK; not Aizome integration or system tracing",
          "sdk_version": "v56.1", "build_command": build, "modes": {},
          "source_sha256": {p.name: hashlib.sha256(p.read_bytes()).hexdigest()
                            for p in [sdk / "perfetto.h", sdk / "perfetto.cc"]}}
for mode in ["lazy-idle", "initialized-idle", "record-then-idle"]:
    p = subprocess.Popen([exe, mode], stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    lines = []
    try:
        while True:
            line = p.stdout.readline().strip()
            if line == "ready":
                break
            if not line and p.poll() is not None:
                raise RuntimeError(p.stderr.read())
            lines.append(line)
        time.sleep(2)
        before = probes.snapshot(p.pid)
        start = time.monotonic()
        time.sleep(8)
        after = probes.snapshot(p.pid)
        result["modes"][mode] = {"before": before, "after": after,
                                  "seconds": time.monotonic() - start, "stdout": lines}
    finally:
        p.terminate()
        p.communicate(timeout=5)
    print(mode, result["modes"][mode], flush=True)
result["traces"] = {}
for trace in sorted(Path.cwd().glob("capture-*.pftrace")):
    query = "select name, count(*) as n from slice group by name order by name"
    output = probes.run([sdk / "trace_processor", trace, "-Q", query])
    assert '"synthetic-stage",1000' in output, output
    assert 'disabled-between-sessions' not in output, output
    result["traces"][trace.name] = {"bytes": trace.stat().st_size,
                                    "sha256": hashlib.sha256(trace.read_bytes()).hexdigest(),
                                    "slice_query": output}
Path("perfetto-results.json").write_text(json.dumps(result, indent=2) + "\n")
