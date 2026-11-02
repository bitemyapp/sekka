#!/usr/bin/env python3
"""Summarize terminal-matrix runs into summary.json and Markdown tables.

Usage: terminal-summary.py DATA_ROOT [OUTPUT_DIR]

DATA_ROOT holds matrix outputs named by phase: startup*/, steady*/,
throughput*/, energy*/, idle-default*/ (several per phase are pooled). Only
accepted trials count; the rest are listed with their reasons. Medians with
min/max (and quartiles where there are enough samples).
"""
import json
from pathlib import Path
import statistics
import sys

FRAME_MS = 1000 / 60  # the private output's refresh


def stats(values):
    values = [v for v in values if v is not None]
    if not values:
        return None
    ordered = sorted(values)
    result = {"n": len(values), "median": statistics.median(values), "min": ordered[0], "max": ordered[-1]}
    if len(values) >= 4:
        q = statistics.quantiles(values, n=4)
        result.update(p25=q[0], p75=q[2])
    return result


def tail(values):
    values = sorted(values)
    q = statistics.quantiles(values, n=100, method="inclusive")
    return {"n": len(values), "p50": statistics.median(values), "p95": q[94], "p99": q[98], "max": values[-1],
            "mean": statistics.fmean(values)}


def accepted_runs(root, phase):
    runs, rejected = [], []
    for matrix in sorted(root.glob(phase + "*")):
        if not (matrix / "result.json").exists():
            continue
        report = json.loads((matrix / "result.json").read_text())
        if report.get("phase") != phase:
            continue
        for trial in report["trials"]:
            path = matrix / f"{trial['round']:03}-{trial['app']}" / f"try{trial['attempt']}" / "run/result.json"
            if trial.get("accepted"):
                runs.append((trial, json.loads(path.read_text()), matrix.name))
            elif "accepted" in trial:
                rejected.append({"matrix": matrix.name, "round": trial["round"], "app": trial["app"], "attempt": trial["attempt"],
                                 "reasons": trial.get("rejection_reasons")})
    return runs, rejected


def uss(total):
    return total["Private_Clean"] + total["Private_Dirty"]


def memory_stats(samples):
    return {"pss_mib": stats([m["total_kib"]["Pss"] / 1024 for m in samples]),
            "rss_mib": stats([m["total_kib"]["Rss"] / 1024 for m in samples]),
            "uss_mib": stats([uss(m["total_kib"]) / 1024 for m in samples]),
            "processes": stats([len(m["processes"]) for m in samples]),
            "drm_kib": {key: stats([m["drm_kib"].get(key, 0) for m in samples]) for key in sorted({k for m in samples for k in m["drm_kib"]})}}


def baseline_w(run):
    p = run["power"]
    values = [p[name]["mean_w"] for name in ("baseline_pre", "baseline_post") if name in p and p[name]["mean_w"]]
    return statistics.fmean(values) if values else None


def window_stats(runs, name):
    entries = []
    for _, run, _ in runs:
        w, p = run["windows"].get(name), run["power"].get(name)
        if not w or not p:
            continue
        base = baseline_w(run)
        seconds = w["roles"]["terminal"]["seconds"]
        entries.append({
            "mean_w": p["mean_w"], "baseline_w": base, "delta_w": p["mean_w"] - base if base else None,
            "energy_j": p["energy_j"], "terminal_cpu_ms_per_s": w["roles"]["terminal"]["cpu_ms_sched"] / seconds,
            "terminal_cpu_ticks_ms_per_s": w["roles"]["terminal"]["cpu_ms_ticks"] / seconds,
            "terminal_ctxt_per_s": w["roles"]["terminal"]["ctxt_per_second"],
            "terminal_voluntary_per_s": w["roles"]["terminal"]["ctxt_voluntary"] / seconds,
            "terminal_threads": w["roles"]["terminal"]["threads"],
            "compositor_cpu_ms_per_s": w["roles"]["compositor"]["cpu_ms_sched"] / seconds,
            "compositor_ctxt_per_s": w["roles"]["compositor"]["ctxt_per_second"],
            "fixture_cpu_ms_per_s": w["roles"]["fixture"]["cpu_ms_sched"] / seconds,
            "terminal_gpu_ms_per_s": w.get("gpu_ns", {}).get("terminal", 0) / 1e6 / seconds,
            "compositor_gpu_ms_per_s": w.get("gpu_ns", {}).get("compositor", 0) / 1e6 / seconds,
            "seconds": seconds})
    if not entries:
        return None
    return {key: stats([e[key] for e in entries]) for key in entries[0]}


def main():
    root = Path(sys.argv[1]).resolve()
    out = Path(sys.argv[2]).resolve() if len(sys.argv) > 2 else root
    summary = {"schema": 2, "frame_ms": FRAME_MS, "apps": {}, "rejected": {}, "versions": {}}
    apps = summary["apps"]

    def entry(app):
        return apps.setdefault(app, {})

    for phase in ("startup", "steady", "throughput", "energy", "idle-default"):
        runs, rejected = accepted_runs(root, phase)
        summary["rejected"][phase] = rejected
        by_app = {}
        for trial, run, matrix in runs:
            by_app.setdefault(trial["app"], []).append((trial, run, matrix))
        for app, items in by_app.items():
            e = entry(app)
            results = [r for _, r, _ in items]
            e.setdefault("trials", {})[phase] = len(results)
            e.setdefault("quiet_starts", {})[phase] = sum(1 for t, _, _ in items if t.get("quiet_start"))
            for r in results:
                summary["versions"].setdefault(app, {"binary": r["binary"],
                                                     "terminal_executables_sha256": r.get("terminal_executables_sha256")})
            if phase != "idle-default":
                e.setdefault("startup_pooled", {"pty_ms": [], "frame_ms": []})
                for r in results:
                    e["startup_pooled"]["pty_ms"].append(r["startup"]["launch_to_pty_ready_ns"] / 1e6)
                    e["startup_pooled"]["frame_ms"].append(r["startup"]["launch_to_matching_capture_ns"] / 1e6)
            if phase == "startup":
                e["startup"] = {"launch_to_pty_ready_ms": stats([r["startup"]["launch_to_pty_ready_ns"] / 1e6 for r in results]),
                                "launch_to_first_frame_ms": stats([r["startup"]["launch_to_matching_capture_ns"] / 1e6 for r in results]),
                                "initial_grid_matched": sum(1 for r in results if r["startup"]["initial_grid_matches_requested"]),
                                "initial_grids": sorted({tuple(r["startup"]["initial_grid"]) for r in results}),
                                "energy_j": stats([r["startup"]["power"]["energy_j"] for r in results if r["startup"].get("power")])}
                e["memory_after_startup"] = memory_stats([r["memory_after_startup"] for r in results])
                # What the compositor holds meanwhile: its own memory and the
                # GPU memory its textures of the terminal's buffers take.
                e["compositor_memory_after_startup"] = memory_stats([r["memory_compositor"] for r in results])
                e["geometry"] = {"cell": sorted({(r["geometry"]["cell_width"], r["geometry"]["cell_height"]) for r in results}),
                                 "window_rect": results[0].get("window_rect"), "grid": results[0].get("grid")}
                e["gpu"] = {"renderer_log": results[0]["gpu"].get("renderer_log"),
                            "application": results[0]["gpu"].get("application"),
                            "compositor": results[0]["gpu"].get("compositor_log")}
            if phase == "steady":
                latencies = [k["inject_to_capture_ns"] / 1e6 for r in results for k in r["keyboard"]]
                to_read = [k["inject_to_fixture_read_ns"] / 1e6 for r in results for k in r["keyboard"]]
                to_frame = [k["fixture_write_to_capture_ns"] / 1e6 for r in results for k in r["keyboard"]]
                t = tail(latencies)
                e["keyboard_ms"] = t
                e["keyboard_frames"] = {k: (v / FRAME_MS if k != "n" else v) for k, v in t.items()}
                e["keyboard_parts_ms"] = {"inject_to_fixture_read": tail(to_read), "fixture_write_to_capture": tail(to_frame)}
                e["keyboard_per_trial_p50_ms"] = [statistics.median([k["inject_to_capture_ns"] / 1e6 for k in r["keyboard"]]) for r in results]
                streams = {}
                for r in results:
                    for s in r["streams"]:
                        key = f"{s['kind']} {s['producer_hz'] or 'unpaced'}"
                        streams.setdefault(key, []).append(s)
                e["streams"] = {key: {"produced": items[0]["produced"], "seen": [s["observed_unique"] for s in items],
                                      "terminal_cpu_ms": stats([s["roles"]["terminal"]["cpu_ms_sched"] for s in items]),
                                      "compositor_cpu_ms": stats([s["roles"]["compositor"]["cpu_ms_sched"] for s in items]),
                                      "final_write_to_capture_ms": stats([s["final_write_to_capture_ns"] / 1e6 for s in items])}
                                for key, items in streams.items()}
            if phase == "throughput":
                corpora = {}
                for r in results:
                    for t in r["throughput"]:
                        name = Path(t["corpus"]).stem
                        p, pre = r["power"].get("tp:" + name), r["power"].get(t["pre_window"])
                        seconds = (t["final_capture_ns"] - t["write_start_ns"]) / 1e9
                        corpora.setdefault(name, []).append({
                            "consume_ms": t["consume_ns"] / 1e6, "mib_per_second": t["mib_per_second"],
                            "to_final_frame_ms": t["to_final_capture_ns"] / 1e6,
                            "terminal_cpu_ms": t["roles"]["terminal"]["cpu_ms_sched"],
                            "compositor_cpu_ms": t["roles"]["compositor"]["cpu_ms_sched"],
                            "fixture_cpu_ms": t["roles"]["fixture"]["cpu_ms_sched"],
                            "terminal_gpu_ms": t["gpu_ns"]["terminal"] / 1e6, "compositor_gpu_ms": t["gpu_ns"]["compositor"] / 1e6,
                            "energy_j": p["energy_j"] if p else None, "mean_w": p["mean_w"] if p else None,
                            "baseline_w": pre["mean_w"] if pre else None,
                            "energy_over_baseline_j": p["energy_j"] - pre["mean_w"] * seconds if p and pre else None,
                            "bytes": t["bytes"]})
                e["throughput"] = {name: {key: stats([i[key] for i in items]) for key in items[0] if key != "bytes"}
                                   | {"bytes": items[0]["bytes"]} for name, items in corpora.items()}
                e["memory_after_throughput"] = memory_stats([r["memory_after_throughput"] for r in results])
            if phase == "energy":
                e["idle_matched"] = window_stats(items, "idle")
                e["typing"] = window_stats(items, "typing")
                e["baseline_w_energy_trials"] = stats([baseline_w(r) for r in results])
                e["memory_idle_matched"] = memory_stats([r["memory_before_idle"] for r in results])
            if phase == "idle-default":
                e["idle_default"] = window_stats(items, "idle")
                e["baseline_w_default_trials"] = stats([baseline_w(r) for r in results])
                e["memory_idle_default"] = memory_stats([r["memory_before_idle"] for r in results])
                e["default_geometry"] = {"cells": sorted({(r["geometry"]["cell_width"], r["geometry"]["cell_height"]) for r in results}),
                                         "grids": sorted({(r["grid"]["cols"], r["grid"]["rows"]) for r in results})}
                e["default_renderer_log"] = results[0]["gpu"].get("renderer_log")
    for e in apps.values():
        if "startup_pooled" in e:
            pooled = e.pop("startup_pooled")
            e["startup_all_matched_launches"] = {"launch_to_pty_ready_ms": stats(pooled["pty_ms"]),
                                                 "launch_to_first_frame_ms": stats(pooled["frame_ms"])}
    (out / "summary.json").write_text(json.dumps(summary, indent=1, default=list) + "\n")
    print_tables(summary, out)


def fmt(value, digits=1):
    if value is None:
        return "–"
    return f"{value:,.{digits}f}"


def med(s, digits=1, spread=True):
    if not s:
        return "–"
    if spread and s["n"] > 1:
        return f"{fmt(s['median'], digits)} ({fmt(s['min'], digits)}–{fmt(s['max'], digits)})"
    return fmt(s["median"], digits)


ORDER = ("hotaru", "ghostty", "gnome-terminal", "konsole", "kgx", "foot", "alacritty", "kitty")
LABELS = {"hotaru": "Hotaru", "ghostty": "Ghostty", "gnome-terminal": "gnome-terminal", "konsole": "Konsole",
          "kgx": "Console (kgx)", "foot": "foot", "alacritty": "Alacritty", "kitty": "kitty"}


def print_tables(summary, out=None):
    """Markdown tables, one per measurement, terminals as columns."""
    apps = summary["apps"]
    names = [n for n in ORDER if n in apps] + sorted(n for n in apps if n not in ORDER)
    lines = []

    def table(title, rows):
        lines.append(f"### {title}\n")
        lines.append("| | " + " | ".join(LABELS.get(n, n) for n in names) + " |")
        lines.append("|---|" + "---:|" * len(names))
        for label, getter in rows:
            cells = []
            for name in names:
                try:
                    cells.append(getter(apps[name]))
                except (KeyError, TypeError, IndexError):
                    cells.append("–")
            lines.append(f"| {label} | " + " | ".join(cells) + " |")
        lines.append("")

    table("Startup (warm caches)", [
        ("Launch → PTY ready, ms", lambda a: med(a["startup"]["launch_to_pty_ready_ms"])),
        ("Launch → first frame, ms", lambda a: med(a["startup"]["launch_to_first_frame_ms"])),
        ("trials", lambda a: str(a["startup"]["launch_to_first_frame_ms"]["n"])),
        ("first frame, all matched launches", lambda a: f"{fmt(a['startup_all_matched_launches']['launch_to_first_frame_ms']['median'])}"
                                                    f" (n={a['startup_all_matched_launches']['launch_to_first_frame_ms']['n']})")])
    table("Key to frame", [
        ("presses", lambda a: str(a["keyboard_ms"]["n"])),
        ("p50, ms", lambda a: fmt(a["keyboard_ms"]["p50"])), ("p95, ms", lambda a: fmt(a["keyboard_ms"]["p95"])),
        ("p99, ms", lambda a: fmt(a["keyboard_ms"]["p99"])), ("max, ms", lambda a: fmt(a["keyboard_ms"]["max"])),
        ("p50, frames (60 Hz)", lambda a: fmt(a["keyboard_frames"]["p50"], 2)),
        ("p99, frames", lambda a: fmt(a["keyboard_frames"]["p99"], 2)),
        ("key → PTY read, p50 ms", lambda a: fmt(a["keyboard_parts_ms"]["inject_to_fixture_read"]["p50"], 2)),
        ("PTY write → frame, p50 ms", lambda a: fmt(a["keyboard_parts_ms"]["fixture_write_to_capture"]["p50"], 2))])
    rows = []
    for key in ("marker 30", "marker 60", "sparse 30", "sparse 60", "scroll 30", "scroll 60"):
        rows.append((f"{key} Hz: updates seen", lambda a, key=key: f"{min(a['streams'][key]['seen'])}–{max(a['streams'][key]['seen'])}"
                                                                     f"/{a['streams'][key]['produced']}"))
        rows.append((f"{key} Hz: terminal CPU, ms", lambda a, key=key: med(a["streams"][key]["terminal_cpu_ms"], 0)))
        rows.append((f"{key} Hz: sway CPU, ms", lambda a, key=key: med(a["streams"][key]["compositor_cpu_ms"], 0)))
    table("Paced updates (120 per stream)", rows)
    rows = []
    for corpus in ("ascii", "styled", "unicode", "cursor"):
        rows.append((f"{corpus}: ms", lambda a, c=corpus: med(a["throughput"][c]["consume_ms"], 0)))
        rows.append((f"{corpus}: MiB/s", lambda a, c=corpus: fmt(a["throughput"][c]["mib_per_second"]["median"], 0)))
        rows.append((f"{corpus}: to final frame, ms", lambda a, c=corpus: fmt(a["throughput"][c]["to_final_frame_ms"]["median"], 0)))
        rows.append((f"{corpus}: terminal CPU, ms", lambda a, c=corpus: fmt(a["throughput"][c]["terminal_cpu_ms"]["median"], 0)))
        rows.append((f"{corpus}: sway CPU, ms", lambda a, c=corpus: fmt(a["throughput"][c]["compositor_cpu_ms"]["median"], 0)))
        rows.append((f"{corpus}: GPU (terminal + sway), ms", lambda a, c=corpus: fmt(a["throughput"][c]["terminal_gpu_ms"]["median"]
                                                                                     + a["throughput"][c]["compositor_gpu_ms"]["median"], 0)))
        rows.append((f"{corpus}: energy, J", lambda a, c=corpus: med(a["throughput"][c]["energy_j"], 1)))
        rows.append((f"{corpus}: over idle baseline, J", lambda a, c=corpus: med(a["throughput"][c]["energy_over_baseline_j"], 1)))
    table("Throughput (64 MiB per corpus)", rows)
    for title, key in (("Idle, matched configuration (60 s)", "idle_matched"), ("Idle, default configuration (60 s)", "idle_default"),
                       ("Typing, 10 keys/s for 60 s", "typing")):
        table(title, [
            ("socket power, W", lambda a, k=key: med(a[k]["mean_w"], 2)),
            ("over baseline, W", lambda a, k=key: med(a[k]["delta_w"], 2)),
            ("terminal CPU, ms/s", lambda a, k=key: med(a[k]["terminal_cpu_ms_per_s"], 2)),
            ("terminal context switches/s", lambda a, k=key: med(a[k]["terminal_ctxt_per_s"], 1)),
            ("terminal threads", lambda a, k=key: fmt(a[k]["terminal_threads"]["median"], 0)),
            ("sway CPU, ms/s", lambda a, k=key: med(a[k]["compositor_cpu_ms_per_s"], 2)),
            ("GPU busy (terminal + sway), ms/s", lambda a, k=key: fmt(a[k]["terminal_gpu_ms_per_s"]["median"]
                                                                       + a[k]["compositor_gpu_ms_per_s"]["median"], 2)),
            ("trials", lambda a, k=key: str(a[k]["mean_w"]["n"]))])
    table("Memory", [
        ("PSS after startup, MiB", lambda a: med(a["memory_after_startup"]["pss_mib"])),
        ("RSS after startup, MiB", lambda a: med(a["memory_after_startup"]["rss_mib"])),
        ("USS after startup, MiB", lambda a: med(a["memory_after_startup"]["uss_mib"])),
        ("GPU memory (DRM, terminal), MiB", lambda a: fmt((a["memory_after_startup"]["drm_kib"].get("drm-total-vram", {"median": 0})["median"]
                                                          + a["memory_after_startup"]["drm_kib"].get("drm-total-gtt", {"median": 0})["median"]) / 1024)),
        ("sway's GPU memory meanwhile, MiB", lambda a: fmt((a["compositor_memory_after_startup"]["drm_kib"].get("drm-total-vram", {"median": 0})["median"]
                                                           + a["compositor_memory_after_startup"]["drm_kib"].get("drm-total-gtt", {"median": 0})["median"]) / 1024)),
        ("PSS after throughput, MiB", lambda a: med(a["memory_after_throughput"]["pss_mib"])),
        ("RSS after throughput, MiB", lambda a: med(a["memory_after_throughput"]["rss_mib"])),
        ("PSS idle, default config, MiB", lambda a: med(a["memory_idle_default"]["pss_mib"]))])
    text = "\n".join(lines)
    print(text)
    if out:
        (out / "summary.md").write_text(text + "\n")


if __name__ == "__main__":
    main()
