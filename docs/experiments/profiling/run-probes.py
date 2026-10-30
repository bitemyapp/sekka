#!/usr/bin/env python3
"""Run small, nonprivileged research fixtures on wx-workstation.

Usage: run-probes.py --tracy SOURCE --output NEW_DIRECTORY
Requires rustc, g++, binutils, and Python on PATH. Never changes kernel policy.
All results are synthetic component evidence, not a battery/desktop benchmark.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import random
import re
import statistics
import subprocess
import time


def run(args, **kw):
    return subprocess.run([str(x) for x in args], check=True, capture_output=True, text=True, **kw).stdout


def snapshot(pid):
    base = Path(f"/proc/{pid}")
    smaps = (base / "smaps_rollup").read_text()
    ticks = 0
    switches = 0
    tasks = list((base / "task").iterdir())
    for task in tasks:
        stat = (task / "stat").read_text().rsplit(")", 1)[1].split()
        ticks += int(stat[11]) + int(stat[12])
        status = (task / "status").read_text()
        switches += sum(int(x) for x in re.findall(r"(?:non)?voluntary_ctxt_switches:\s+(\d+)", status))
    return {
        "pss_kib": int(re.search(r"^Pss:\s+(\d+)", smaps, re.M)[1]),
        "rss_kib": int(re.search(r"^Rss:\s+(\d+)", smaps, re.M)[1]),
        "threads": len(tasks), "cpu_ticks": ticks, "context_switches": switches,
    }


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--tracy", type=Path, required=True)
    ap.add_argument("--output", type=Path, required=True)
    a = ap.parse_args()
    a.output.mkdir(parents=True, exist_ok=False)
    a.output = a.output.resolve()
    source = Path(__file__).resolve().parent
    a.tracy = a.tracy.resolve()
    result = {"scope": "synthetic, unprivileged workstation fixtures; no desktop or energy claim",
              "utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
              "kernel": platform.release(), "rustc": run(["rustc", "-Vv"]),
              "compiler": run(["g++", "--version"]).splitlines()[0],
              "clock_ticks_per_second": os.sysconf("SC_CLK_TCK"),
              "load_start": os.getloadavg(), "commands": []}

    def command(args, **kw):
        result["commands"].append([str(x) for x in args])
        return run(args, **kw)

    # Split debug data from the exact binary, preserving code and unwind tables.
    original = a.output / "probe"
    command(["rustc", source / "probe.rs", "--edition=2024", "-C", "opt-level=3",
             "-C", "debuginfo=line-tables-only", "-C", "codegen-units=4",
             "-C", "link-arg=-Wl,--build-id=sha1", "-o", original])
    debug = a.output / "probe.debug"
    runtime = a.output / "probe.runtime"
    command(["objcopy", "--only-keep-debug", original, debug])
    command(["objcopy", "--strip-debug", f"--add-gnu-debuglink={debug}", original, runtime])
    sizes = {p.name: p.stat().st_size for p in [original, runtime, debug]}
    sections = {}
    ids = {}
    for p in [original, runtime]:
        ids[p.name] = re.search(r"Build ID: (\w+)", command(["readelf", "-n", p]))[1]
        sections[p.name] = {}
        for section in [".text", ".eh_frame"]:
            dest = a.output / (p.name + section)
            command(["objcopy", "--dump-section", f"{section}={dest}", p])
            sections[p.name][section] = hashlib.sha256(dest.read_bytes()).hexdigest()
    nm = command(["nm", "-n", original])
    address = next(line.split()[0] for line in nm.splitlines() if "interaction_stage" in line)
    lines = command(["addr2line", "-f", "-C", "-e", debug, address])
    assert "probe.rs:" in lines, lines
    assert ids[original.name] == ids[runtime.name]
    assert sections[original.name] == sections[runtime.name]
    assert command([original]) == command([runtime])
    result["split_debug"] = {"sizes_bytes": sizes, "build_ids": ids,
                             "section_sha256": sections, "source_resolution": lines}

    configs = {
        "compiled_out": [],
        "on_demand": ["TRACY_ENABLE", "TRACY_ON_DEMAND", "TRACY_NO_BROADCAST", "TRACY_ONLY_LOCALHOST"],
        "on_demand_no_system": ["TRACY_ENABLE", "TRACY_ON_DEMAND", "TRACY_NO_BROADCAST",
                                "TRACY_ONLY_LOCALHOST", "TRACY_NO_SYSTEM_TRACING", "TRACY_NO_SAMPLING"],
        "manual_not_started": ["TRACY_ENABLE", "TRACY_ON_DEMAND", "TRACY_NO_BROADCAST",
                               "TRACY_ONLY_LOCALHOST", "TRACY_NO_SYSTEM_TRACING", "TRACY_NO_SAMPLING",
                               "TRACY_DELAYED_INIT", "TRACY_MANUAL_LIFETIME"],
    }
    result["tracy"] = {}
    # Serialize builds; CPU usage and disk footprint stay small.
    for name, defines in configs.items():
        exe = a.output / name
        args = ["g++", "-std=c++17", "-O2", "-pthread", f"-I{a.tracy / 'public'}"]
        args += [f"-D{x}" for x in defines]
        args += [source / "tracy-probe.cpp"]
        if defines:
            args += [a.tracy / "public/TracyClient.cpp"]
        command(args + ["-ldl", "-o", exe])
        p = subprocess.Popen([exe, "idle"], stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        try:
            assert p.stdout.readline().strip() == "ready"
            time.sleep(2)
            before = snapshot(p.pid)
            start = time.monotonic()
            time.sleep(8)
            after = snapshot(p.pid)
            elapsed = time.monotonic() - start
        finally:
            p.terminate()
            p.communicate(timeout=5)
        result["tracy"][name] = {"defines": defines, "binary_bytes": exe.stat().st_size,
                                  "before": before, "after": after, "seconds": elapsed,
                                  "cpu_seconds": (after["cpu_ticks"] - before["cpu_ticks"]) / os.sysconf("SC_CLK_TCK"),
                                  "context_switches_delta": after["context_switches"] - before["context_switches"],
                                  "ns_per_zone_runs": []}
        print(name, result["tracy"][name], flush=True)
    # Interleave variants deterministically, pinning to one allowed CPU. These
    # tiny-loop timings are deliberately not estimates of application overhead.
    cpu = min(os.sched_getaffinity(0))
    names = list(configs) * 3
    random.Random(42).shuffle(names)
    for name in names:
        rows = command(["taskset", "-c", cpu, a.output / name]).splitlines()
        result["tracy"][name]["ns_per_zone_runs"].extend(float(v) for v in rows)
    for item in result["tracy"].values():
        item["ns_per_zone_median"] = statistics.median(item["ns_per_zone_runs"])
    result["affinity_cpu"] = cpu
    result["load_end"] = os.getloadavg()
    (a.output / "results.json").write_text(json.dumps(result, indent=2) + "\n")
    print(a.output / "results.json")


if __name__ == "__main__":
    main()
