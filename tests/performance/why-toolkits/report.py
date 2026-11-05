#!/usr/bin/env python3
"""Tables of the why-toolkits summaries (analyze.py's JSON) in Markdown.

    report.py SUMMARY.json [SUMMARY.json ...] > tables.md"""
import json
import sys

NAMES = {"kit": "Kit", "gtk4": "GTK 4", "adw": "libadwaita", "qtwidgets": "Qt Widgets", "qtquick": "Qt Quick",
         "hotaru": "Hotaru", "foot": "foot", "sumi": "Sumi", "gte": "GNOME Text Editor", "kwrite": "KWrite",
         "hako": "Hako", "nautilus": "Files (Nautilus)", "dolphin": "Dolphin",
         "aizome-kit": "Kit on Aizome", "gnome-gtk4": "GTK 4 on GNOME Shell", "gnome-adw": "libadwaita on GNOME Shell",
         "kde-qtwidgets": "Qt Widgets on KWin", "kde-qtquick": "Qt Quick on KWin",
         "aizome-sumi": "Sumi on Aizome", "gnome-gte": "GNOME Text Editor on GNOME Shell", "kde-kwrite": "KWrite on KWin"}
ORDER = list(NAMES)


def f(v, d=2):
    if v is None:
        return "–"
    if isinstance(v, dict):
        v = v.get("median", v.get("p50"))
        if v is None:
            return "–"
    if abs(v) >= 1000:
        return f"{v:,.0f}"
    return f"{v:.{d}f}"


def table(title, rows, cols):
    out = [f"### {title}", "", "| | " + " | ".join(c[0] for c in cols) + " |", "|---|" + "---:|" * len(cols)]
    for name, s in rows:
        out.append(f"| {NAMES.get(name, name)} | " + " | ".join(c[1](s) for c in cols) + " |")
    return "\n".join(out) + "\n"


def g(*path):
    def get(s):
        v = s
        for p in path:
            if v is None:
                return None
            v = v.get(p) if isinstance(v, dict) else None
        return v
    return get


def main():
    summary = {}
    for path in sys.argv[1:]:
        for sc, subs in json.load(open(path))["summary"].items():
            summary.setdefault(sc, {}).update(subs)
    out = []
    for sc in sorted(summary):
        subs = summary[sc]
        rows = sorted(subs.items(), key=lambda kv: ORDER.index(kv[0]) if kv[0] in ORDER else 99)
        first = rows[0][1]
        if "latency_ms" in first:
            cols = [("p50 ms", lambda s: f(g("latency_ms", "p50")(s))), ("p90 ms", lambda s: f(g("latency_ms", "p90")(s))),
                    ("p99 ms", lambda s: f(g("latency_ms", "p99")(s))), ("max ms", lambda s: f(g("latency_ms", "max")(s))),
                    ("p50 frames", lambda s: f(g("latency_frames", "p50")(s))),
                    ("keys", lambda s: str(g("latency_ms", "n")(s))), ("missed", lambda s: str(s.get("missed")))]
        elif "key_commit" in first:
            cols = [("deliver p50", lambda s: f(g("deliver", "p50")(s), 3)),
                    ("key→commit p50 / p90", lambda s: f(g("key_commit", "p50")(s), 3) + " / " + f(g("key_commit", "p90")(s), 3)),
                    ("commit→done p50", lambda s: f(g("commit_done", "p50")(s))),
                    ("key→done p50 / p99", lambda s: f(g("key_done", "p50")(s)) + " / " + f(g("key_done", "p99")(s))),
                    ("key→presented p50", lambda s: f(g("key_presented", "p50")(s))),
                    ("frames/key", lambda s: f(g("frames_per_key", "mean")(s))),
                    ("damage px p50", lambda s: f(g("area_px", "p50")(s), 0)), ("buffers", lambda s: s.get("kinds", "")),
                    ("app CPU ms/key", lambda s: f(s.get("app_cpu_ms_per_key"), 3)),
                    ("comp CPU ms/key", lambda s: f(s.get("comp_cpu_ms_per_key"), 3)),
                    ("app GPU ms/key", lambda s: f(s.get("app_gpu_ms_per_key"), 3)),
                    ("comp GPU ms/key", lambda s: f(s.get("comp_gpu_ms_per_key"), 3)),
                    ("app wakeups/key", lambda s: f(s.get("app_wakeups_per_key"), 1)),
                    ("mJ/key", lambda s: f(s.get("energy_mj_per_key"), 1))]
        else:
            cols = [("fps", lambda s: f(s.get("fps"), 1)), ("long gaps", lambda s: f(s.get("long_intervals"), 0)),
                    ("p99 gap ms", lambda s: f(s.get("interval_p99_ms"))),
                    ("damage px/frame", lambda s: f(s.get("area_px_per_frame"), 0)),
                    ("buffers", lambda s: s.get("kinds", "")),
                    ("shm MB/frame", lambda s: f((g("shm_bytes_per_frame", "median")(s) or 0) / 1e6, 2)),
                    ("app CPU ms/frame", lambda s: f(s.get("app_cpu_ms_per_frame"), 3)),
                    ("comp CPU ms/frame", lambda s: f(s.get("comp_cpu_ms_per_frame"), 3)),
                    ("app GPU ms/frame", lambda s: f(s.get("app_gpu_ms_per_frame"), 3)),
                    ("comp GPU ms/frame", lambda s: f(s.get("comp_gpu_ms_per_frame"), 3)),
                    ("app CPU ms/s", lambda s: f(s.get("app_cpu_ms_per_s"), 1)),
                    ("comp CPU ms/s", lambda s: f(s.get("comp_cpu_ms_per_s"), 1)),
                    ("app wakeups/s", lambda s: f(s.get("app_wakeups_per_s"), 1)),
                    ("ΔW", lambda s: f(s.get("power_delta_w"), 2)),
                    ("cfg→commit p50 ms", lambda s: f(s.get("configure_to_commit_p50_ms"), 2))]
        out.append(table(sc, rows, cols))
        n = {k: v.get("trials") for k, v in subs.items()}
        out.append(f"Trials: {n}\n")
    print("\n".join(out))


if __name__ == "__main__":
    main()
