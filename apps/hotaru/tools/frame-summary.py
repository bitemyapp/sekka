#!/usr/bin/env python3
"""Summarize one host's frame-matrix runs: a startup and a steady directory.

Usage: frame-summary.py DIRECTORY [LABEL]

DIRECTORY holds the `startup/` and `steady/` outputs of tools/frame-matrix.py.
Every trial must have been accepted. Writes DIRECTORY/summary.json and prints
the comparison; the raw results stay where they are.
"""
from pathlib import Path
import json
import statistics
import sys


def ms(ns):
    return ns / 1_000_000


def distribution(values):
    return {"median": statistics.median(values), "min": min(values), "max": max(values), "samples": values}


def cpu_ms(before, after, role, ticks):
    return (after[role]["cpu_ticks"] - before[role]["cpu_ticks"]) / ticks * 1000


def main():
    root = Path(sys.argv[1]).resolve()
    label = sys.argv[2] if len(sys.argv) > 2 else root.name
    summary = {"schema": 1, "label": label, "startup": {}, "steady": {}, "gates": {}, "gpu": {}}
    for phase in ("startup", "steady"):
        matrix = json.loads((root / phase / "result.json").read_text())
        if matrix["status"] != "passed" or not all(t["accepted"] for t in matrix["trials"]):
            raise SystemExit(f"{phase}: matrix {matrix['status']}, or a trial was rejected")
        summary["gates"][phase] = {"trials": len(matrix["trials"]), "renderer": matrix.get("renderer", "software"),
                                   "cpu_sets": matrix["cpu_sets"], "sources": matrix["sources"],
                                   "preflight_attempts": [t["preflight"].get("attempts", 1) for t in matrix["trials"]],
                                   "compiler_overlap_samples": sum(t["compiler_overlap_samples"] for t in matrix["trials"])}
        for kind in ("hotaru", "ghostty"):
            results = [json.loads(p.read_text()) for p in sorted((root / phase).glob(f"*-{kind}/run/result.json"))]
            identities = {(r["running_executable"], r["running_executable_sha256"]) for r in results}
            if len(identities) != 1:
                raise SystemExit(f"{phase} {kind}: more than one executable")
            executable, sha = identities.pop()
            entry = {"executable": executable, "sha256": sha, "trials": len(results)}
            gpu = [r.get("gpu", {}) for r in results]
            summary["gpu"].setdefault(kind, {"compositor": gpu[0].get("compositor", {}).get("log"),
                                             "presentation": [g.get("ghostty", {}).get("gpu_presentation") for g in gpu],
                                             "application": next(iter(gpu[0].get("application", {}).values()), None)})
            if phase == "startup":
                for name in ("launch_to_pty_ready_ns", "launch_to_matching_capture_ns"):
                    entry[name.replace("_ns", "_ms")] = distribution([ms(r["startup"][name]) for r in results])
            else:
                latencies = [ms(k["inject_to_capture_ns"]) for r in results for k in r["keyboard"]]
                entry["keyboard_ms"] = {"n": len(latencies), "p50": statistics.median(latencies),
                                        "p95": statistics.quantiles(latencies, n=100)[94],
                                        "p99": statistics.quantiles(latencies, n=100)[98], "max": max(latencies)}
                ticks = results[0]["clock_ticks_per_second"]
                idle = [c for r in results for c in r["observer_controls"] if not c["active"]]
                entry["idle"] = {"seconds": [round((c["end_ns"] - c["start_ns"]) / 1e9, 1) for c in idle],
                                 "app_cpu_ms": [round(cpu_ms(c["before"], c["after"], "application", ticks)) for c in idle],
                                 "app_rss_mib": [c["after"]["application"]["memory_kib_except_threads"]["VmRSS"] // 1024 for c in idle]}
                streams = {}
                for r in results:
                    for s in r["streams"]:
                        key = f"{s['kind']} {s['producer_hz'] or 'unpaced'}"
                        streams.setdefault(key, []).append({
                            "seen": s["observed_unique"], "produced": s["produced"],
                            "app_cpu_ms": round(cpu_ms(s["before"], s["after"], "application", ticks)),
                            "compositor_cpu_ms": round(cpu_ms(s["before"], s["after"], "compositor", ticks)),
                            "final_write_to_capture_ms": round(ms(s["final_write_to_capture_ns"]), 2)})
                entry["streams"] = streams
            summary[phase][kind] = entry
    throughput = root / "throughput" / "result.json"
    if throughput.exists():
        matrix = json.loads(throughput.read_text())
        if matrix["status"] != "passed" or not all(t["accepted"] for t in matrix["trials"]):
            raise SystemExit(f"throughput: matrix {matrix['status']}, or a trial was rejected")
        summary["gates"]["throughput"] = {"trials": len(matrix["trials"]), "cpu_sets": matrix["cpu_sets"],
                                          "preflight_attempts": [t["preflight"].get("attempts", 1) for t in matrix["trials"]]}
        summary["throughput"] = {}
        for kind in ("hotaru", "ghostty"):
            runs = {}
            for path in sorted((root / "throughput").glob(f"*-{kind}/run/result.json")):
                for t in json.loads(path.read_text())["throughput"]:
                    runs.setdefault(Path(t["corpus"]).stem, []).append(t)
            summary["throughput"][kind] = {corpus: {
                "bytes": ts[0]["bytes"], "trials": len(ts),
                "consume_ms": distribution([ms(t["consume_ns"]) for t in ts]),
                "mib_per_second": distribution([t["mib_per_second"] for t in ts]),
                "to_final_capture_ms": distribution([ms(t["to_final_capture_ns"]) for t in ts]),
                "captures_during": [t["captures_during"] for t in ts],
                "application_cpu_ms": distribution([t["application_cpu_ms"] for t in ts]),
                "compositor_cpu_ms": distribution([t["compositor_cpu_ms"] for t in ts])} for corpus, ts in runs.items()}
    (root / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
    print(f"== {label}")
    for kind in ("hotaru", "ghostty"):
        s, t = summary["startup"][kind], summary["steady"][kind]
        k = t["keyboard_ms"]
        print(f"{kind:8} launch→PTY {s['launch_to_pty_ready_ms']['median']:7.1f} ms  launch→frame "
              f"{s['launch_to_matching_capture_ms']['median']:7.1f} ms  key p50/p95/p99/max "
              f"{k['p50']:.1f}/{k['p95']:.1f}/{k['p99']:.1f}/{k['max']:.1f} ms  idle cpu {t['idle']['app_cpu_ms']} ms "
              f"rss {t['idle']['app_rss_mib']} MiB")
        for key, runs in t["streams"].items():
            print(f"   {key:14} seen {[r['seen'] for r in runs]}/{runs[0]['produced']}  app cpu {[r['app_cpu_ms'] for r in runs]} ms"
                  f"  compositor cpu {[r['compositor_cpu_ms'] for r in runs]} ms")
        for corpus, tp in summary.get("throughput", {}).get(kind, {}).items():
            print(f"   throughput {corpus:8} {tp['bytes'] >> 20} MiB: consumed {tp['consume_ms']['median']:8.1f} ms "
                  f"({tp['mib_per_second']['median']:6.1f} MiB/s), final frame {tp['to_final_capture_ms']['median']:8.1f} ms, "
                  f"app cpu {tp['application_cpu_ms']['median']:7.0f} ms, compositor cpu {tp['compositor_cpu_ms']['median']:5.0f} ms, "
                  f"frames {tp['captures_during']}")


if __name__ == "__main__":
    main()
