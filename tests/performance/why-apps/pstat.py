#!/usr/bin/env python3
"""perf stat from launch to the first frame (probe.py's `pstat` set):
medians over the runs of task-clock, user and system time (rusage: the
kernel's 1 kHz tick sampling, scaled to the exact run time), page faults,
context switches and user-mode instructions.

    pstat.py RUNDIR [--json OUT.json]
"""
import collections
import json
import re
import statistics
import sys


def parse(path):
    out = {}
    try:
        text = open(path).read()
    except OSError:
        return out
    for line in text.splitlines():
        m = re.match(r"^\s*([\d,.]+)\s+(?:msec\s+)?([\w:-]+)", line)
        if m and m.group(2) in ("task-clock", "task-clock:u", "page-faults", "page-faults:u", "major-faults",
                                "major-faults:u", "context-switches", "context-switches:u", "cycles:u",
                                "instructions:u"):
            out[m.group(2).split(":")[0]] = float(m.group(1).replace(",", ""))
        m = re.match(r"^\s*([\d.]+) seconds (user|sys|time elapsed)", line)
        if m:
            out[m.group(2)] = float(m.group(1)) * 1000
    return out


def main():
    rundir = sys.argv[1]
    runs = [json.loads(l) for l in open(f"{rundir}/runs.jsonl")]
    runs = [r for r in runs if not r.get("meta") and not r.get("warmup")]
    by = collections.OrderedDict()
    for r in runs:
        by.setdefault(r["name"], []).append(parse(f"{rundir}/{r['name']}-{r['rep']}.pstat"))
    rows = {}
    print("| Variant | n | task-clock, ms | user, ms | system, ms | page faults | context switches | user instructions, M | IPC (user) |")
    print("|---|---:|---:|---:|---:|---:|---:|---:|---:|")
    for name, ps in by.items():
        ps = [p for p in ps if "task-clock" in p]
        if not ps:
            continue
        med = lambda k: statistics.median(p.get(k, 0.0) for p in ps)
        row = {k: med(k) for k in ("task-clock", "user", "sys", "page-faults", "context-switches", "cycles",
                                   "instructions")}
        row["n"] = len(ps)
        rows[name] = row
        ipc = row["instructions"] / row["cycles"] if row["cycles"] else 0
        print(f"| {name} | {len(ps)} | {row['task-clock']:.1f} | {row['user']:.1f} | {row['sys']:.1f} | "
              f"{row['page-faults']:.0f} | {row['context-switches']:.0f} | {row['instructions'] / 1e6:.0f} | {ipc:.2f} |")
    if len(sys.argv) > 3 and sys.argv[2] == "--json":
        json.dump(rows, open(sys.argv[3], "w"), indent=1)


if __name__ == "__main__":
    main()
