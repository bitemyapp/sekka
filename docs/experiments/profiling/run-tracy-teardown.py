#!/usr/bin/env python3
"""Isolate manual-lifetime teardown after the initial disconnected-state probe.

Usage: run-tracy-teardown.py TRACY_SOURCE NEW_OUTPUT_DIRECTORY
"""
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import time

source = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("probes", source / "run-probes.py")
probes = importlib.util.module_from_spec(spec)
spec.loader.exec_module(probes)
tracy = Path(sys.argv[1]).resolve()
out = Path(sys.argv[2]).resolve()
out.mkdir(parents=True, exist_ok=False)
exe = out / "manual-lifetime"
flags = ["TRACY_ENABLE", "TRACY_ON_DEMAND", "TRACY_NO_BROADCAST", "TRACY_ONLY_LOCALHOST",
         "TRACY_NO_SYSTEM_TRACING", "TRACY_NO_SAMPLING", "TRACY_DELAYED_INIT", "TRACY_MANUAL_LIFETIME"]
cmd = ["g++", "-std=c++17", "-O2", "-pthread", f"-I{tracy / 'public'}"]
cmd += [f"-D{x}" for x in flags]
cmd += [source / "tracy-lifecycle.cpp", tracy / "public/TracyClient.cpp", "-ldl", "-o", exe]
probes.run(cmd)
p = subprocess.Popen([exe], stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
try:
    assert p.stdout.readline().strip() == "ready", p.stderr.read()
    time.sleep(2)
    before = probes.snapshot(p.pid)
    start = time.monotonic()
    time.sleep(8)
    after = probes.snapshot(p.pid)
    result = {"scope": "two disconnected startup/shutdown cycles, no connected capture",
              "command": [str(x) for x in cmd], "before": before, "after": after,
              "seconds": time.monotonic() - start, "ticks_per_second": os.sysconf("SC_CLK_TCK")}
finally:
    p.terminate()
    p.communicate(timeout=5)
(out / "tracy-teardown.json").write_text(json.dumps(result, indent=2) + "\n")
print(json.dumps(result))
