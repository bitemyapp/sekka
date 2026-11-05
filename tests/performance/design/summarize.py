#!/usr/bin/env python3
"""Summarise window-cost.py records: per program, the median and range of
every measure over the measured runs (warm-ups left out).

    summarize.py RECORD.json [RECORD.json ...] > summary.md
    summarize.py --json RECORD.json [...] > summary.json
"""
import json
import statistics
import sys

MEASURES = [
    ("first_frame_ms", "First frame, ms", 1),
    ("cpu_to_first_frame_sched_ms", "CPU to first frame, ms", 1),
    ("pss_kib", "PSS, MiB", 1024),
    ("pss_anon_kib", "Private anon, MiB", 1024),
    ("rss_kib", "RSS, MiB", 1024),
    ("threads", "Threads", 1),
    ("shared_objects", "Shared objects", 1),
    ("idle_switches_per_s", "Wakeups/s idle", 1),
    ("idle_cpu_sched_ms_per_s", "Idle CPU, ms/s", 1),
]


def summarise(path):
    rec = json.load(open(path))
    order, by = [], {}
    for r in rec["runs"]:
        if r.get("warmup") or "error" in r:
            continue
        if r["name"] not in by:
            order.append(r["name"])
            by[r["name"]] = []
        by[r["name"]].append(r)
    out = {}
    for name in order:
        runs = by[name]
        s = {"runs": len(runs), "gpu_driver_libs": sorted({g for r in runs for g in r["gpu_driver_libs"]}),
             "app_id": runs[0]["window"]["app_id"], "geometry": runs[0]["window"]["geometry"]}
        for key, _, div in MEASURES:
            vals = [r[key] / div for r in runs if key in r]
            if key in ("idle_switches_per_s", "idle_cpu_sched_ms_per_s"):
                # Summed over live threads, so a thread that exits during the
                # idle window takes its count with it: below zero means none.
                vals = [max(0.0, v) for v in vals]
            if vals:
                s[key] = {"median": statistics.median(vals), "min": min(vals), "max": max(vals)}
        out[name] = s
    errors = [r for r in rec["runs"] if "error" in r]
    return rec, out, errors


def fmt(v):
    if v >= 100:
        return f"{v:.0f}"
    if v >= 10:
        return f"{v:.1f}"
    return f"{v:.2f}"


def main():
    args = sys.argv[1:]
    as_json = args and args[0] == "--json"
    if as_json:
        args = args[1:]
    everything = {}
    for path in args:
        rec, out, errors = summarise(path)
        everything[path] = {"date": rec["date"], "load_before": rec["load_before"], "load_after": rec.get("load_after"),
                            "reps": rec["reps"], "idle_s": rec["idle_s"], "programs": out,
                            "errors": [{"name": e["name"], "error": e["error"]} for e in errors]}
        if as_json:
            continue
        print(f"### {path}\n")
        print(f"{rec['date']}, {rec['reps']} runs each after a warm-up, load {' '.join(rec['load_before'])} before"
              f" and {' '.join(rec.get('load_after') or [])} after.\n")
        heads = [h for _, h, _ in MEASURES]
        print("| Program | " + " | ".join(heads) + " | GPU driver mapped |")
        print("|---|" + "---:|" * len(heads) + "---|")
        for name, s in out.items():
            cells = []
            for key, _, _ in MEASURES:
                m = s.get(key)
                cells.append(f"{fmt(m['median'])} ({fmt(m['min'])}–{fmt(m['max'])})" if m else "–")
            gpu = "yes" if s["gpu_driver_libs"] else "no"
            print(f"| {name} | " + " | ".join(cells) + f" | {gpu} |")
        if errors:
            print(f"\nErrors: {[(e['name'], e['error']) for e in errors]}")
        print()
    if as_json:
        json.dump(everything, sys.stdout, indent=1)


if __name__ == "__main__":
    main()
