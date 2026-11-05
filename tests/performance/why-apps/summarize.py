#!/usr/bin/env python3
"""Medians of probe.py runs, per variant, and each variant's difference
from its program's base run (the variant named without a suffix).

    summarize.py RUNDIR [--json OUT.json]

Columns: first frame (exec to sway's window `new`), CPU of every thread of
the run at that moment (schedstat), the main thread's CPU and the rest of
its time (first frame less its CPU: waiting, for the compositor, a D-Bus
reply or another thread, or runnable but not running), and the shared
objects mapped at the first frame.
"""
import collections
import json
import statistics
import sys


def med(xs):
    xs = [x for x in xs if x is not None]
    return statistics.median(xs) if xs else None


def main():
    rundir = sys.argv[1]
    out_json = sys.argv[3] if len(sys.argv) > 3 and sys.argv[2] == "--json" else None
    runs = [json.loads(l) for l in open(f"{rundir}/runs.jsonl")]
    meta = [r for r in runs if r.get("meta")]
    runs = [r for r in runs if not r.get("meta") and not r.get("warmup")]
    by = collections.OrderedDict()
    for r in runs:
        by.setdefault(r["name"], []).append(r)
    rows = {}
    for name, rs in by.items():
        ff = [r.get("first_frame_ms") for r in rs]
        cpu, main_cpu, main_wait, objs = [], [], [], []
        for r in rs:
            ths = r.get("threads_at_map") or []
            if not ths:
                continue
            cpu.append(sum(t["run_ns"] for t in ths) / 1e6)
            wp = r.get("window_pid")
            mt = [t for t in ths if t["tid"] == wp] or [t for t in ths if t["tid"] == t["pid"]][:1]
            if mt:
                main_cpu.append(mt[0]["run_ns"] / 1e6)
                main_wait.append(r["first_frame_ms"] - mt[0]["run_ns"] / 1e6)
            objs.append(len(r.get("shared_objects_at_map") or []))
        good = [x for x in ff if x is not None]
        rows[name] = {
            "n": len(good), "failed": len(ff) - len(good),
            "first_frame_ms": med(good), "min": min(good) if good else None, "max": max(good) if good else None,
            "cpu_ms": med(cpu), "main_cpu_ms": med(main_cpu), "main_not_running_ms": med(main_wait),
            "shared_objects": med(objs),
        }
    print(f"{rundir}: {meta[0].get('date') if meta else ''}, load before {meta[0].get('load_before') if meta else ''}, "
          f"after {meta[-1].get('load_after') if len(meta) > 1 else ''}\n")
    print("| Variant | n | First frame, ms (range) | Δ vs base, ms | CPU, all threads, ms | Main thread CPU, ms | Main thread not running, ms | Shared objects |")
    print("|---|---:|---|---:|---:|---:|---:|---:|")
    for name, v in rows.items():
        base = rows.get(name.split(".")[0])
        d = ""
        if base and base is not v and v["first_frame_ms"] is not None and base["first_frame_ms"] is not None:
            d = f"{v['first_frame_ms'] - base['first_frame_ms']:+.1f}"
            v["delta_ms"] = round(v["first_frame_ms"] - base["first_frame_ms"], 2)
        f = lambda x: "–" if x is None else f"{x:.1f}"
        print(f"| {name} | {v['n']}{' (' + str(v['failed']) + ' failed)' if v['failed'] else ''} | "
              f"{f(v['first_frame_ms'])} ({f(v['min'])}–{f(v['max'])}) | {d} | {f(v['cpu_ms'])} | "
              f"{f(v['main_cpu_ms'])} | {f(v['main_not_running_ms'])} | {f(v['shared_objects'])} |")
    if out_json:
        with open(out_json, "w") as fh:
            json.dump({"meta": meta, "variants": rows}, fh, indent=1)


if __name__ == "__main__":
    main()
