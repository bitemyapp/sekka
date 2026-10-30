#!/usr/bin/env python3
"""Feed-only cross-check of the Ghostty-suite inputs.

The pinned libghostty-vt's reference adapter and hotaru-replay each read a file
before timing and time only the feed, with zero history, as tools/benchmark.py
does. They run in four rounds of five iterations, alternating order, pinned
to one CPU under the workstation lock. Whole-command times from
ghostty-bench include reading the input; these don't.
"""
import fcntl
import json
import statistics
import subprocess
import sys
from pathlib import Path

REF = Path("/home/callen/work/ghostty-reference-20261028")
ADAPTERS = {"ghostty": REF / "zig-out/bin/ghostty-reference-replay",
            "hotaru": Path("/home/callen/work/sekka-hotaru-settings/apps/hotaru/target/release/hotaru-replay")}
DATA = Path(sys.argv[1])
OUT = Path(sys.argv[2])
NAMES = sys.argv[3:]

report = {"scope": __doc__, "rows": 80, "cols": 120, "cpu": 8, "inputs": {}}
with Path("/home/callen/.cache/hotaru-workstation.lock").open("a") as lock:
    fcntl.flock(lock, fcntl.LOCK_EX)
    for name in NAMES:
        path = DATA / f"{name}.vt"
        samples = {engine: [] for engine in ADAPTERS}
        for round_id in range(4):
            order = list(ADAPTERS) if round_id % 2 == 0 else list(reversed(ADAPTERS))
            for engine in order:
                result = subprocess.run(["taskset", "-c", "8", str(ADAPTERS[engine]), "--data", str(path),
                                         "--rows", "80", "--cols", "120", "--bench-json", "--iterations", "5"],
                                        capture_output=True, text=True, check=True, timeout=1200)
                samples[engine].extend(json.loads(result.stdout)["samples_ns"])
        medians = {engine: statistics.median(values) / 1e6 for engine, values in samples.items()}
        report["inputs"][name] = {"median_feed_ms": {k: round(v, 2) for k, v in medians.items()},
                                  "hotaru_over_ghostty": round(medians["hotaru"] / medians["ghostty"], 3),
                                  "samples_ns": samples}
        OUT.write_text(json.dumps(report, indent=2) + "\n")
        print(name, report["inputs"][name]["median_feed_ms"], report["inputs"][name]["hotaru_over_ghostty"], flush=True)
