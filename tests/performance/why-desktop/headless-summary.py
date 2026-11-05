#!/usr/bin/env python3
"""Tables from headless.py's records (docs/performance/why-desktop.md).

    headless-summary.py RUNS_DIR OUT.json > tables.md

Clean runs (tags r1, r2, wallpaper on) give the figures; the -nowall runs
the wallpaper's share; -perf runs are not used for any figure here.
"""
import json
import lzma
import os
import statistics as st
import sys

ROLE = {
    "gnome": {"gnome-shell": "compositor+shell"},
    "plasma": {"kwin_wayland": "compositor", "plasmashell": "shell"},
    "aizome": {"aizome": "compositor", "aizome-shell": "shell"},
}
ORDER = ["aizome", "gnome", "plasma"]
NAMES = {"aizome": "Aizome", "gnome": "GNOME", "plasma": "Plasma"}


def load(runs):
    """Records, leaving out GNOME runs whose shell never finished starting
    (its startup waits for the background; with none set it never did)."""
    out = {}
    for d in sorted(os.listdir(runs)):
        f = os.path.join(runs, d, "record.json")
        if os.path.exists(f):
            r = json.load(open(f))
        elif os.path.exists(f + ".xz"):
            r = json.load(lzma.open(f + ".xz", "rt"))
        else:
            continue
        log = os.path.join(runs, d, "session.log")
        text = open(log).read() if os.path.exists(log) else \
            lzma.open(log + ".xz", "rt").read() if os.path.exists(log + ".xz") else None
        if r["desktop"] == "gnome" and text is not None and "GNOME Shell started" not in text:
            print(f"<!-- left out {d}: GNOME Shell did not finish starting -->")
            continue
        out[d] = r
    return out


def thread_label(proc, thread):
    """A process's main thread is named after it (Nix's wrappers make that
    `.NAME-wrapped`, cut to 15 characters)."""
    t = thread.lstrip(".")
    if t == proc or (t.startswith(proc) and "-wrapped".startswith(t[len(proc):])):
        return "main thread"
    return t


def med(xs):
    xs = [x for x in xs if x is not None]
    return st.median(xs) if xs else None


def mains(rec):
    """{role: (memory entry, idle entry, updates entry, move entry)} by process name."""
    desk = rec["desktop"]
    want = ROLE[desk]
    res = {}
    for k, v in rec.get("memory", {}).items():
        if v["name"] in want:
            res.setdefault(want[v["name"]], {})["memory"] = v
    for phase in ("idle", "updates", "move"):
        for k, v in rec.get(phase, {}).get("processes", {}).items():
            if v["name"] in want:
                res.setdefault(want[v["name"]], {})[phase] = v
    return res


def main():
    runs, out_path = sys.argv[1], sys.argv[2]
    recs = load(runs)
    clean = {d: [r for n, r in recs.items() if r["desktop"] == d and not r["perf"] and r["wallpaper"] is True
                 and not r.get("error")] for d in ORDER}
    nowall = {d: [r for n, r in recs.items() if r["desktop"] == d and not r["perf"] and r["wallpaper"] is False
                  and not r.get("error")] for d in ORDER}
    ownwall = {d: [r for n, r in recs.items() if r["desktop"] == d and not r["perf"]
                   and isinstance(r["wallpaper"], str) and not r.get("error")] for d in ORDER}
    summary = {}
    print("### Compositor and shell memory by mapping, headless on the host (MiB PSS; median of clean runs)\n")
    cats = set()
    per = {}
    for d in ORDER:
        for r in clean[d]:
            for role, m in mains(r).items():
                for c in m["memory"].get("categories", {}):
                    cats.add(c)
    for d in ORDER:
        for r in clean[d]:
            for role, m in mains(r).items():
                key = (d, role)
                for c in cats:
                    v = m["memory"].get("categories", {}).get(c, {}).get("pss_kib", 0) / 1024
                    per.setdefault(key, {}).setdefault(c, []).append(v)
                per.setdefault(key, {}).setdefault("_total", []).append(m["memory"]["rollup"]["Pss"] / 1024)
                per.setdefault(key, {}).setdefault("_anon", []).append(m["memory"]["rollup"]["Pss_Anon"] / 1024)
    cols = [(d, role) for d in ORDER for role in sorted(set(ROLE[d].values()), key=lambda x: (x != "compositor", x))
            if (d, role) in per]
    print("| Mapping | " + " | ".join(f"{NAMES[d]} {role}" for d, role in cols) + " |")
    print("|---|" + "---:|" * len(cols))
    rows = sorted(cats, key=lambda c: -max(med(per[k].get(c, [0])) or 0 for k in cols))
    for c in rows:
        vals = [med(per[k].get(c, [0])) or 0 for k in cols]
        if max(vals) < 0.5:
            continue
        print(f"| {c} | " + " | ".join(f"{v:.1f}" for v in vals) + " |")
    print("| **PSS total** | " + " | ".join(f"**{med(per[k]['_total']):.1f}**" for k in cols) + " |")
    print("| of which anonymous | " + " | ".join(f"{med(per[k]['_anon']):.1f}" for k in cols) + " |")
    summary["memory_by_mapping"] = {f"{d}/{role}": {c: round(med(v), 2) for c, v in per[(d, role)].items()}
                                    for d, role in cols}
    # wallpaper share
    print("\n### The wallpaper's share (MiB PSS of the compositor and shell; medians)\n")
    print("| | Sekka's wallpaper | With it | Against | Difference | of it anonymous |")
    print("|---|---|---:|---:|---:|---:|")
    walls = {"aizome": "Tokyo at dusk, 6000 × 3750", "gnome": "Foggy forest, 4928 × 3264",
             "plasma": "Tatami ocean wave (look-and-feel)"}
    summary["wallpaper"] = {}
    def tot(rs, key):
        return med([sum(m["memory"]["rollup"][key] for m in mains(r).values()) / 1024 for r in rs])
    for d in ORDER:
        plain = "Plasma's default look and feel and its wallpaper" if d == "plasma" else "no picture"
        for other, label in ((nowall, plain), (ownwall, "the same picture at 1920 × 1080")):
            if not other[d] or not clean[d]:
                continue
            a, b = tot(clean[d], "Pss"), tot(other[d], "Pss")
            aa, ba = tot(clean[d], "Pss_Anon"), tot(other[d], "Pss_Anon")
            print(f"| {NAMES[d]} | {walls[d]} | {a:.1f} | {b:.1f} ({label}) | {a - b:+.1f} | {aa - ba:+.1f} |")
            summary["wallpaper"][f"{d} vs {label}"] = dict(with_mib=round(a, 1), against_mib=round(b, 1),
                                                           pss_delta_mib=round(a - b, 1), anon_delta_mib=round(aa - ba, 1))
    # session totals
    print("\n### The whole headless session (MiB PSS; median of clean runs)\n")
    print("| | Compositor + shell | Everything else the session started | Total | Processes |")
    print("|---|---:|---:|---:|---:|")
    summary["session"] = {}
    for d in ORDER:
        if not clean[d]:
            continue
        cs, other, n = [], [], []
        for r in clean[d]:
            want = ROLE[d]
            c = sum(v["rollup"]["Pss"] for v in r["memory"].values() if v["name"] in want) / 1024
            o = sum(v["rollup"]["Pss"] for v in r["memory"].values()
                    if v["name"] not in want and v["name"] not in ("bwrap", "dbus-daemon")) / 1024
            cs.append(c)
            other.append(o)
            n.append(len(r["memory"]))
        print(f"| {NAMES[d]} | {med(cs):.1f} | {med(other):.1f} | {med(cs) + med(other):.1f} | {med(n):.0f} |")
        summary["session"][d] = dict(compositor_shell=round(med(cs), 1), other=round(med(other), 1))
    # biggest other processes (largest median)
    print("\n### Largest other processes of the headless sessions (MiB PSS, anonymous; one clean run)\n")
    for d in ORDER:
        if not clean[d]:
            continue
        r = clean[d][0]
        rows = sorted((v for v in r["memory"].values() if v["name"] not in ROLE[d]),
                      key=lambda v: -v["rollup"]["Pss"])[:8]
        print(f"- {NAMES[d]}: " + "; ".join(f"{v['name']} {v['rollup']['Pss'] / 1024:.0f} ({v['rollup']['Pss_Anon'] / 1024:.0f})"
                                            for v in rows))
    # CPU and wakeups
    print("\n### Work, headless on the host (two CPUs; median of clean runs)\n")
    print("| | Idle: wakeups/s | Idle: CPU ms/s | One update: CPU ms | One update: wakeups | One move step: CPU ms |")
    print("|---|---:|---:|---:|---:|---:|")
    summary["work"] = {}
    for d in ORDER:
        for role in sorted(set(ROLE[d].values()), key=lambda x: (x != "compositor", x)):
            iw, ic, uc, uw, mc = [], [], [], [], []
            for r in clean[d]:
                m = mains(r).get(role, {})
                if "idle" in m:
                    iw.append(m["idle"]["wakeups_per_s"])
                    ic.append(m["idle"]["cpu_ms_per_s"])
                if "updates" in m:
                    secs, cnt = r["updates"]["seconds"], r["updates"]["count"]
                    uc.append(m["updates"]["cpu_ms_per_s"] * secs / cnt)
                    uw.append(m["updates"]["wakeups_per_s"] * secs / cnt)
                if "move" in m:
                    secs, cnt = r["move"]["seconds"], r["move"]["steps"]
                    mc.append(m["move"]["cpu_ms_per_s"] * secs / cnt)
            if not iw:
                continue
            row = dict(idle_wakeups_per_s=med(iw), idle_cpu_ms_per_s=med(ic), update_cpu_ms=med(uc),
                       update_wakeups=med(uw), move_step_cpu_ms=med(mc))
            summary["work"][f"{d}/{role}"] = row
            f = lambda v, p=2: "–" if v is None else f"{v:.{p}f}"
            print(f"| {NAMES[d]} {role} | {f(row['idle_wakeups_per_s'], 1)} | {f(row['idle_cpu_ms_per_s'], 3)} | "
                  f"{f(row['update_cpu_ms'])} | {f(row['update_wakeups'], 1)} | {f(row['move_step_cpu_ms'])} |")
    # CPU per update and per move step, by thread
    print("\n### Where an update's and a move step's CPU goes, by thread (ms; mean of clean runs)\n")
    print("| | One update | One move step |")
    print("|---|---|---|")
    summary["by_thread"] = {}
    for d in ORDER:
        cells = []
        for phase, key in (("updates", "count"), ("move", "steps")):
            agg = {}
            for r in clean[d]:
                secs, n = r[phase]["seconds"], r[phase][key]
                for v in r[phase]["processes"].values():
                    for t in v["threads"]:
                        agg.setdefault((v["name"], thread_label(v["name"], t["thread"])), []).append(
                            t["cpu_ms_per_s"] * secs / n)
            rows = sorted(((sum(x) / len(clean[d]), k) for k, x in agg.items()), reverse=True)
            rows = [(v, k) for v, k in rows if v >= 0.03][:5]
            summary["by_thread"][f"{d}/{phase}"] = {f"{k[0]}/{k[1]}": round(v, 3) for v, k in rows}
            cells.append("; ".join(f"{k[0]}: {k[1]} {v:.2f}" for v, k in rows))
        print(f"| {NAMES[d]} | {cells[0]} | {cells[1]} |")
    # top idle threads
    print("\n### Who wakes at idle, headless (wakeups/s by thread; one clean run)\n")
    for d in ORDER:
        if not clean[d]:
            continue
        r = clean[d][0]
        for role, m in mains(r).items():
            groups = {}
            for t in m.get("idle", {}).get("threads", []):
                g = groups.setdefault(thread_label(m["idle"]["name"], t["thread"]), [0, 0.0])
                g[0] += 1
                g[1] += t["wakeups_per_s"]
            rows = sorted(groups.items(), key=lambda kv: -kv[1][1])[:8]
            print(f"- {NAMES[d]} {role}: " + "; ".join(
                (f"{k} ({n} threads) {w:.1f}" if n > 1 else f"{k} {w:.1f}") for k, (n, w) in rows if w >= 0.1))
    json.dump(summary, open(out_path, "w"), indent=1)


if __name__ == "__main__":
    main()
