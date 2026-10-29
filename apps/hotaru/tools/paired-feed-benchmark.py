#!/usr/bin/env python3
"""Paired feed-only timings of two hotaru-replay builds on the same inputs.

Usage: paired-feed-benchmark.py BASELINE CANDIDATE REPORT [--cpu N]

Each case first checks that both builds end in the same terminal state, then
alternates them over rounds on one pinned CPU after warmups. The report gives
each build's median feed time and their ratio (candidate / baseline; below 1
is faster), with load averages and CPU frequencies recorded, not gated. This
is a paired comparison of two engines, not a whole-application measurement.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import statistics
import subprocess
import tempfile

from corpus import PATTERNS

ROUNDS, ITERATIONS, WARMUP = 6, 10, 3


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def frequency(cpu):
    try:
        return int(Path(f"/sys/devices/system/cpu/cpu{cpu}/cpufreq/scaling_cur_freq").read_text())
    except (OSError, ValueError):
        return None


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("baseline", type=Path)
    parser.add_argument("candidate", type=Path)
    parser.add_argument("report", type=Path)
    parser.add_argument("--cpu", type=int, default=120)
    args = parser.parse_args()
    builds = [args.baseline.resolve(), args.candidate.resolve()]
    report = {"baseline": {"path": str(builds[0]), "sha256": sha(builds[0])},
              "candidate": {"path": str(builds[1]), "sha256": sha(builds[1])},
              "cpu": args.cpu, "rounds": ROUNDS, "iterations_per_round": ITERATIONS, "warmup_iterations": WARMUP,
              "scope": "feed-only, zero history, 120 columns, 65536-byte chunks", "cases": []}
    with tempfile.TemporaryDirectory() as directory:
        for name in ["ascii", "unicode", "styled", "cursor"]:
            pattern = PATTERNS[name]
            for size, rows in [(2 * 1024 * 1024, 12), (8 * 1024 * 1024, 80)]:
                path = Path(directory) / f"{name}-{size}.vt"
                path.write_bytes(pattern * ((size + len(pattern) - 1) // len(pattern)))
                for mode in ["legacy", "unicode"]:
                    params = ["--data", str(path), "--rows", str(rows), "--cols", "120", "--chunk", "65536",
                              "--grapheme-width", mode]
                    states = [subprocess.check_output([str(b), *params, "--format", "json"]) for b in builds]
                    assert states[0] == states[1], (name, size, mode, "the builds end in different states")

                    def run(build, iterations):
                        out = subprocess.check_output(["taskset", "-c", str(args.cpu), str(build), *params,
                                                       "--bench-json", "--iterations", str(iterations)])
                        return json.loads(out)["samples_ns"]

                    for build in builds:
                        run(build, WARMUP)
                    samples = [[], []]
                    for round_index in range(ROUNDS):
                        for index in ([0, 1] if round_index % 2 == 0 else [1, 0]):
                            samples[index] += run(builds[index], ITERATIONS)
                    medians = [statistics.median(s) for s in samples]
                    case = {"input": name, "bytes": path.stat().st_size, "rows": rows, "mode": mode,
                            "median_ns": medians, "ratio": medians[1] / medians[0],
                            "samples_ns": samples, "load_average": os.getloadavg(),
                            "frequency_khz": frequency(args.cpu)}
                    report["cases"].append(case)
                    print(f"{name:8} {size >> 20} MiB {mode:7} ratio {case['ratio']:.4f}", flush=True)
    ratios = [case["ratio"] for case in report["cases"]]
    report["ratio_geometric_mean"] = statistics.geometric_mean(ratios)
    report["ratio_worst"] = max(ratios)
    args.report.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps({key: report[key] for key in ("ratio_geometric_mean", "ratio_worst")}))


if __name__ == "__main__":
    main()
