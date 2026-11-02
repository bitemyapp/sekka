#!/usr/bin/env python3
"""Summarise bench.py trials into docs/performance/data/desktops.

    summarize.py [--runs DIR] [--out DIR]

Reads every runs/<variant>-<mem>/<tag>/result.json (and the raw files next
to it), writes per-trial summaries, an aggregate summary.json, Markdown
tables (tables.md), and copies the raw files worth keeping (compressed).
"""
import argparse
import json
import lzma
import os
import re
import shutil
import statistics
from collections import defaultdict
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[2]
ORDER = ["aizome", "gnome", "plasma", "baseline"]
TITLES = {"aizome": "Aizome", "gnome": "GNOME", "plasma": "Plasma", "baseline": "No desktop"}


def pct(values, p):
    v = sorted(values)
    if not v:
        return None
    k = (len(v) - 1) * p / 100
    f = int(k)
    c = min(f + 1, len(v) - 1)
    return v[f] + (v[c] - v[f]) * (k - f)


def spread(values, digits=1):
    values = [x for x in values if x is not None]
    if not values:
        return None
    return {"n": len(values), "median": round(statistics.median(values), digits), "min": round(min(values), digits),
            "max": round(max(values), digits)}


# ---------------------------------------------------------------- parsing


def meminfo(path):
    kv = {}
    for line in Path(path).read_text().splitlines():
        m = re.match(r"(\w+):\s+(\d+)", line)
        if m:
            kv[m.group(1)] = int(m.group(2))
    psi = re.search(r"some avg10=\S+ avg60=\S+ avg300=\S+ total=(\d+)", Path(path).read_text())
    vm = dict(re.findall(r"^(pswpin|pswpout|zswpin|zswpout|zswpwb) (\d+)$", Path(path).read_text(), re.M))
    return {"used_mib": (kv["MemTotal"] - kv["MemAvailable"]) / 1024,
            "memtotal_mib": kv["MemTotal"] / 1024,
            "available_mib": kv["MemAvailable"] / 1024,
            "anon_mib": (kv.get("AnonPages", 0)) / 1024,
            "shmem_mib": kv.get("Shmem", 0) / 1024,
            "cached_mib": kv.get("Cached", 0) / 1024,
            "swap_used_mib": (kv["SwapTotal"] - kv["SwapFree"]) / 1024,
            "zswap_pool_mib": kv.get("Zswap", 0) / 1024,
            "zswapped_mib": kv.get("Zswapped", 0) / 1024,
            "memory_pressure_some_total_us": int(psi.group(1)) if psi else None,
            "pswpout": int(vm.get("pswpout", 0)), "zswpout": int(vm.get("zswpout", 0))}


def classify(cg, uid, comm):
    """Group a process by where systemd put it."""
    if cg == "/init.scope":
        return "system", "systemd (PID 1)"
    m = re.match(r"/system\.slice/(?:[^/]+\.slice/)*([^/]+)", cg)
    if m:
        return "system", m.group(1)
    m = re.match(r"/user\.slice/user-(\d+)\.slice/(.*)", cg)
    if m:
        who = "session" if m.group(1) == "1000" else "greeter"
        rest = m.group(2)
        unit = rest.split("/")[-1] if rest else rest
        if rest.startswith("user@"):
            parts = rest.split("/")
            unit = parts[-1] if len(parts) > 1 else parts[0]
        return who, unit
    return "other", cg or comm


def processes(path):
    rows = []
    for line in Path(path).read_text().splitlines():
        f = line.split("\t")
        if len(f) < 13:
            continue
        pid, ppid, uid, comm, cg, rss, pss, pa, pf, ps, sw, swp, a0 = f[:13]
        group, unit = classify(cg, uid, comm)
        rows.append({"pid": int(pid), "uid": uid, "comm": comm, "cgroup": cg, "rss_kib": int(rss),
                     "pss_kib": int(pss), "pss_anon_kib": int(pa), "pss_file_kib": int(pf), "pss_shmem_kib": int(ps),
                     "swap_kib": int(sw), "swap_pss_kib": int(swp), "argv0": a0, "group": group, "unit": unit})
    return rows


def memory_summary(run, name):
    mi = meminfo(run / f"meminfo-{name}.txt")
    rows = processes(run / f"mem-{name}.tsv")
    groups = defaultdict(int)
    swap_groups = defaultdict(int)
    for r in rows:
        if "perf-agent" in r["cgroup"]:
            continue
        groups[r["group"]] += r["pss_kib"]
        swap_groups[r["group"]] += r["swap_pss_kib"]
    by_unit = defaultdict(int)
    rows = [r for r in rows if "perf-agent" not in r["cgroup"]]
    for r in rows:
        by_unit[f"{r['group']}:{r['unit']}"] += r["pss_kib"]
    top = sorted(rows, key=lambda r: -r["pss_kib"])[:25]
    return {
        **{k: round(v, 1) for k, v in mi.items() if v is not None and isinstance(v, float)},
        **{k: v for k, v in mi.items() if isinstance(v, int)},
        "pss_mib": {k: round(v / 1024, 1) for k, v in groups.items()},
        "pss_total_mib": round(sum(groups.values()) / 1024, 1),
        "swap_pss_mib": {k: round(v / 1024, 1) for k, v in swap_groups.items()},
        "processes": len(rows),
        "top_units_mib": {k: round(v / 1024, 1) for k, v in sorted(by_unit.items(), key=lambda kv: -kv[1])[:30]},
        "top_processes": [{"comm": r["comm"], "group": r["group"], "unit": r["unit"], "pss_mib": round(r["pss_kib"] / 1024, 1),
                           "swap_pss_mib": round(r["swap_pss_kib"] / 1024, 1)} for r in top],
    }


def tasks(path):
    out = {}
    for line in Path(path).read_text().splitlines():
        f = line.split("\t")
        if len(f) < 7:
            continue
        out[f[0]] = (f[1], int(f[2]), int(f[3]), int(f[4]), int(f[5]), f[6])
    return out


def idle_summary(run, res):
    idle = res["idle"]
    s0, s1 = idle["stat"]
    c0, c1 = s0["cpu"], s1["cpu"]
    d = [b - a for a, b in zip(c0, c1)]
    total = sum(d[:8])
    idle_j = d[3] + d[4]
    busy = (total - idle_j) / total * 100 if total else None
    secs = idle["host_seconds"]
    t0 = tasks(run / "idle-tasks-start.tsv")
    t1 = tasks(run / "idle-tasks-end.tsv")
    per = defaultdict(lambda: [0, 0, 0])
    for k, v1 in t1.items():
        v0 = t0.get(k)
        if not v0 or "perf-agent" in v1[5]:
            continue
        pid = k.split("/")[0]
        key = (v1[0] if True else pid)
        per[(pid, v1[0], v1[5])][0] += (v1[1] - v0[1])
        per[(pid, v1[0], v1[5])][1] += (v1[2] - v0[2])
        per[(pid, v1[0], v1[5])][2] += (v1[3] + v1[4] - v0[3] - v0[4])
    # Aggregate threads to processes by pid, named by the process's main thread.
    procs = defaultdict(lambda: [0, 0, 0, "", ""])
    names = {k.split("/")[0]: v[0] for k, v in t1.items() if k.split("/")[0] == k.split("/")[2]}
    for (pid, comm, cg), (vol, nv, ticks) in per.items():
        p = procs[pid]
        p[0] += vol
        p[1] += nv
        p[2] += ticks
        p[3] = names.get(pid, comm)
        p[4] = cg
    top = sorted(procs.items(), key=lambda kv: -(kv[1][0] + kv[1][1]))[:15]
    top_cpu = sorted(procs.items(), key=lambda kv: -kv[1][2])[:10]
    q0, q1 = idle["qemu_cpu_start"], idle["qemu_cpu_end"]
    qd = {k: q1[k] - q0.get(k, 0) for k in q1}
    vcpu = sum(v for k, v in qd.items() if "/KVM" in k)
    power = idle.get("power", {})
    return {
        "window_s": secs,
        "guest_busy_pct": round(busy, 2) if busy is not None else None,
        "guest_busy_pct_of_one_vcpu": round(busy * 2, 2) if busy is not None else None,
        "ctxt_per_s": round((s1["ctxt"] - s0["ctxt"]) / secs, 1),
        "intr_per_s": round((s1["intr"] - s0["intr"]) / secs, 1),
        "forks_per_s": round((s1["processes"] - s0["processes"]) / secs, 2),
        "task_wakeups_per_s": round(sum(v[0] for v in procs.values()) / secs, 1),
        "qemu_cpu_pct_of_core": round(sum(qd.values()) / secs * 100, 2),
        "qemu_vcpu_pct_of_core": round(vcpu / secs * 100, 2),
        "ppt_mean_w": power.get("mean_w"),
        "ppt_energy_j": power.get("energy_j"),
        "host_busy_cpu_s": power.get("system_busy_cpu_s"),
        "top_wakers": [{"comm": p[3], "cgroup": p[4], "voluntary_per_s": round(p[0] / secs, 2),
                        "involuntary_per_s": round(p[1] / secs, 2), "cpu_pct": round(p[2] / 100 / secs * 100, 3)}
                       for _, p in top],
        "top_cpu": [{"comm": p[3], "cgroup": p[4], "cpu_pct": round(p[2] / 100 / secs * 100, 3)} for _, p in top_cpu],
    }


def read_tsv(path):
    lines = [l for l in Path(path).read_text().splitlines() if l and not l.startswith("#")]
    head = lines[0].split("\t")
    return [dict(zip(head, map(int, l.split("\t")))) for l in lines[1:]]


def latency_summary(run, name):
    p = run / f"{name}.tsv"
    i = run / f"{name}.inject.tsv"
    if not p.exists() or not i.exists():
        return None
    header = Path(p).read_text().splitlines()[0]
    rec = read_tsv(p)
    inj = read_tsv(i)
    n = min(len(rec), len(inj))
    lat, inp, comp, refresh = [], [], [], []
    mism = 0
    discarded = 0
    for r, k in zip(rec[:n], inj[:n]):
        # The key event's own timestamp (ms) must agree with the injection.
        if abs(r["key_ms"] - (k["press_ns"] // 1_000_000) % (1 << 32)) > 1000:
            mism += 1
        if r["state"] != 1:
            discarded += 1
            continue
        lat.append((r["present_ns"] - k["press_ns"]) / 1e6)
        inp.append((r["recv_ns"] - k["press_ns"]) / 1e6)
        comp.append((r["present_ns"] - r["commit_ns"]) / 1e6)
        if r["refresh_ns"]:
            refresh.append(r["refresh_ns"])
    rms = statistics.median(refresh) / 1e6 if refresh else 1000 / 60
    stats = lambda xs: {"p50": round(pct(xs, 50), 2), "p95": round(pct(xs, 95), 2), "p99": round(pct(xs, 99), 2),
                        "max": round(max(xs), 2), "mean": round(statistics.mean(xs), 2)} if xs else None
    s = stats(lat)
    return {
        "presses_injected": len(inj), "presses_received": len(rec), "presented": len(lat), "not_presented": discarded,
        "timestamp_mismatches": mism, "refresh_ms": round(rms, 3), "header": header,
        "key_to_present_ms": s,
        "key_to_present_frames": {k: round(v / rms, 2) for k, v in s.items()} if s else None,
        "key_to_client_ms": stats(inp), "commit_to_present_ms": stats(comp),
        "raw_ms": [round(x, 3) for x in lat],
    }


def pacing_summary(run, name):
    p = run / f"{name}.tsv"
    if not p.exists():
        return None
    rec = read_tsv(p)
    pres = [r for r in rec if r["state"] == 1]
    if len(pres) < 3:
        return {"frames": len(pres)}
    refresh = [r["refresh_ns"] for r in pres if r["refresh_ns"]]
    rms = statistics.median(refresh) / 1e6 if refresh else 1000 / 60
    t = [r["present_ns"] for r in pres]
    iv = [(b - a) / 1e6 for a, b in zip(t, t[1:])]
    dur = (t[-1] - t[0]) / 1e9
    late = sum(1 for x in iv if x > 1.5 * rms)
    # Frames the display had time for but didn't get: each interval of k
    # refreshes skips k-1.
    missed = sum(max(0, round(x / rms) - 1) for x in iv)
    return {
        "commits": len(rec), "presented": len(pres), "discarded": sum(1 for r in rec if r["state"] == 2),
        "no_buffer": sum(1 for r in rec if r["state"] == 3),
        "duration_s": round(dur, 2), "fps": round((len(pres) - 1) / dur, 2) if dur else None, "refresh_ms": round(rms, 3),
        "interval_ms": {"p50": round(pct(iv, 50), 2), "p95": round(pct(iv, 95), 2), "p99": round(pct(iv, 99), 2),
                        "max": round(max(iv), 2)},
        "late_intervals": late, "late_pct": round(late / len(iv) * 100, 2), "missed_refreshes": missed,
        "missed_pct": round(missed / (missed + len(iv)) * 100, 2),
    }


def journal_login(run, res):
    """In-guest login timeline from the journal (guest CLOCK_MONOTONIC)."""
    path = run / "journal.json.xz"
    if not path.exists():
        return None
    events = []
    with lzma.open(path, "rt") as f:
        for line in f:
            try:
                e = json.loads(line)
            except json.JSONDecodeError:
                continue
            events.append(e)
    def mono(e):
        return int(e["__MONOTONIC_TIMESTAMP"]) / 1e6
    pam = [e for e in events if "session opened for user demo" in str(e.get("MESSAGE", ""))]
    if not pam:
        return None
    t_pam = mono(pam[0])
    out = {"pam_service": pam[0].get("SYSLOG_IDENTIFIER"), "pam_session_opened_s": round(t_pam, 3), "units": {}}
    want = {
        "aizome": ["aizome.service", "aizome-shell.service", "graphical-session.target", "aizome-session.target"],
        "gnome": ["org.gnome.Shell@wayland.service", "gnome-session-manager@gnome.service", "gnome-session.target",
                  "graphical-session.target", "gnome-session-wayland@gnome.target"],
        "plasma": ["plasma-kwin_wayland.service", "plasma-plasmashell.service", "plasma-workspace.target",
                   "graphical-session.target", "plasma-core.target"],
    }.get(res["variant"], [])
    for e in events:
        if mono(e) < t_pam:
            continue
        unit = e.get("USER_UNIT") or e.get("UNIT")
        if e.get("_UID") not in ("1000", None) and e.get("_SYSTEMD_USER_UNIT") is None and not e.get("USER_UNIT"):
            pass
        if unit in want and e.get("JOB_TYPE") == "start" and unit not in out["units"]:
            out["units"][unit] = round(mono(e) - t_pam, 3)
    # Last user-unit start job finished within 60 s of the PAM session.
    starts = [mono(e) - t_pam for e in events
              if e.get("JOB_TYPE") == "start" and e.get("USER_UNIT") and 0 <= mono(e) - t_pam < 60]
    out["last_user_unit_started_s"] = round(max(starts), 3) if starts else None
    off = res.get("clock", {}).get("offset_s")
    if off is not None and res.get("login"):
        # Host Enter press on the guest clock, relative to QEMU start.
        enter_guest = res["login"]["host_enter"] + off
        out["enter_to_pam_s"] = round(t_pam - enter_guest, 3)
    return out


def analyze_text(run):
    t = (run / "systemd-analyze.txt").read_text() if (run / "systemd-analyze.txt").exists() else ""
    m = re.search(r"Startup finished in (.*?)\n", t)
    parts = {}
    if m:
        for val, name in re.findall(r"([\d.]+(?:min )?[\d.]*m?s) \((\w+)\)", m.group(1)):
            parts[name] = seconds(val)
    m2 = re.search(r"(graphical|multi-user)\.target reached after ([\dmin .]+s) in userspace", t)
    if m2:
        parts["target_reached_userspace"] = seconds(m2.group(2))
    dm = re.search(r"display-manager\.service @([\d.]+m?s)(?: \+([\d.]+m?s))?", t)
    if dm:
        parts["display_manager_at"] = seconds(dm.group(1))
    return parts


def seconds(text):
    text = text.strip()
    total = 0.0
    for val, unit in re.findall(r"([\d.]+)(min|ms|s)", text):
        total += float(val) * {"min": 60, "s": 1, "ms": 0.001}[unit]
    return round(total, 3)


def unit_times(run, res):
    path = run / "units.txt"
    if not path.exists():
        return {}
    off = res.get("clock", {}).get("offset_s")
    t0 = res.get("clock", {}).get("host_t0")
    out = {}
    cur = None
    for line in path.read_text().splitlines():
        if line.startswith("Id="):
            cur = line[3:]
            # display-manager.service is an alias of greetd's and Plasma
            # Login's units; it was the first unit asked for.
            if not out and cur not in ("multi-user.target", "graphical.target"):
                cur = "display-manager.service"
        elif line.startswith("ActiveEnterTimestampMonotonic=") and cur:
            us = int(line.split("=")[1])
            if us and off is not None:
                out[cur] = round(us / 1e6 - off - t0, 3)
    return out


def apps_summary(res):
    out = {}
    for key, a in (res.get("apps") or {}).items():
        runs = a["runs"]
        cold = runs[0]
        warm = runs[1:]
        ok = lambda r: not r["timeout"] and r["settled"] is not None
        out[key] = {
            "title": a["title"], "argv": a["argv"],
            "cold_settled_s": cold["settled"] if ok(cold) else None,
            "cold_first_change_s": cold["first_change"],
            "cold_pss_mib": round(cold["app_pss_kib"] / 1024, 1),
            "cold_other_new_pss_mib": round(cold["other_new_pss_kib"] / 1024, 1),
            "warm_settled_s": [r["settled"] for r in warm if ok(r)],
            "warm_first_change_s": [r["first_change"] for r in warm],
            "warm_pss_mib": [round(r["app_pss_kib"] / 1024, 1) for r in warm],
            "timeouts": sum(1 for r in runs if r["timeout"]),
        }
    return out


def settled(w):
    """When the final picture was up: the last frame-to-frame change, or,
    if later, the first frame matching the reference (a slow fade changes
    little between two polls but a lot in all)."""
    if not w or w.get("timeout"):
        return None
    return round(max(w.get("settled") or 0, w.get("first_match") or 0), 4)


def trial_summary(run):
    res = json.loads((run / "result.json").read_text())
    s = {"variant": res["variant"], "tag": res["tag"], "mem_mib": res["mem_mib"], "start": res["host"]["start"],
         "loadavg_start": res["host"]["quiet_wait"]["loadavg"], "host_idle_power": res.get("host_idle_power")}
    b = res.get("boot", {})
    s["boot"] = {"settled_s": settled(b), "last_change_s": b.get("settled"), "first_match_s": b.get("first_match"),
                 "timeout": b.get("timeout"), "systemd": analyze_text(run), "units_host_s": unit_times(run, res)}
    if res.get("login"):
        l = res["login"]
        s["login"] = {"settled_s": settled(l), "last_change_s": l.get("settled"),
                      "first_change_s": l.get("first_change"), "first_match_s": l.get("first_match"),
                      "timeout": l.get("timeout"), "journal": journal_login(run, res)}
    s["memory"] = {}
    for name in ("greeter", "desktop60", "desktop190"):
        if (run / f"meminfo-{name}.txt").exists():
            s["memory"][name] = memory_summary(run, name)
    if res.get("idle"):
        s["idle"] = idle_summary(run, res)
        hip = (res.get("host_idle_power") or {}).get("mean_w")
        if hip and s["idle"]["ppt_mean_w"]:
            s["idle"]["ppt_over_host_idle_w"] = round(s["idle"]["ppt_mean_w"] - hip, 2)
    if res.get("responsiveness"):
        s["responsiveness"] = {}
        for name in ("latency-idle", "latency-load"):
            s["responsiveness"][name] = latency_summary(run, name)
        for name in ("pacing-idle", "pacing-load"):
            s["responsiveness"][name] = pacing_summary(run, name)
        s["responsiveness"]["probe_status"] = {k: v.get("status") for k, v in res["responsiveness"].items()
                                               if isinstance(v, dict)}
    if res.get("apps"):
        s["apps"] = apps_summary(res)
    return s


# ---------------------------------------------------------------- aggregate


def aggregate(trials):
    by = defaultdict(list)
    for t in trials:
        by[(t["variant"], t["mem_mib"])].append(t)
    agg = {}
    for (v, mem), ts in sorted(by.items()):
        a = {"trials": [t["tag"] for t in ts]}
        a["boot_s"] = spread([t["boot"]["settled_s"] for t in ts], 2)
        a["boot_kernel_s"] = spread([t["boot"]["systemd"].get("kernel") for t in ts], 2)
        a["boot_initrd_s"] = spread([t["boot"]["systemd"].get("initrd") for t in ts], 2)
        a["boot_userspace_s"] = spread([t["boot"]["systemd"].get("userspace") for t in ts], 2)
        a["boot_display_manager_host_s"] = spread([t["boot"]["units_host_s"].get("display-manager.service") for t in ts], 2)
        if any("login" in t for t in ts):
            a["login_s"] = spread([t["login"]["settled_s"] for t in ts if "login" in t], 2)
            a["login_first_change_s"] = spread([t["login"]["first_change_s"] for t in ts if "login" in t], 2)
            js = [t["login"]["journal"] for t in ts if t.get("login", {}).get("journal")]
            a["login_enter_to_pam_s"] = spread([j.get("enter_to_pam_s") for j in js], 2)
            a["login_pam_to_last_unit_s"] = spread([j.get("last_user_unit_started_s") for j in js], 2)
            units = defaultdict(list)
            for j in js:
                for u, x in j["units"].items():
                    units[u].append(x)
            a["login_pam_to_unit_s"] = {u: spread(x, 2) for u, x in units.items()}
        for name in ("greeter", "desktop60", "desktop190"):
            ms = [t["memory"][name] for t in ts if name in t["memory"]]
            if not ms:
                continue
            a[f"mem_{name}"] = {
                "used_mib": spread([m["used_mib"] for m in ms]),
                "swap_used_mib": spread([m["swap_used_mib"] for m in ms]),
                "zswap_pool_mib": spread([m["zswap_pool_mib"] for m in ms]),
                "pss_total_mib": spread([m["pss_total_mib"] for m in ms]),
                "pss_mib": {g: spread([m["pss_mib"].get(g, 0) for m in ms]) for g in ("system", "greeter", "session", "other")},
                "processes": spread([m["processes"] for m in ms], 0),
            }
        # Where the memory is, by systemd unit (median over trials).
        for name in ("greeter", "desktop190"):
            units = defaultdict(list)
            ms = [t["memory"][name] for t in ts if name in t["memory"]]
            for m in ms:
                for u, x in m["top_units_mib"].items():
                    units[u].append(x)
            a[f"units_{name}_mib"] = dict(sorted(((u, round(statistics.median(x + [0] * (len(ms) - len(x))), 1))
                                                  for u, x in units.items()), key=lambda kv: -kv[1])[:15])
        ids = [t["idle"] for t in ts if t.get("idle")]
        if ids:
            wak = defaultdict(list)
            for i in ids:
                for w in i["top_wakers"]:
                    wak[w["comm"]].append(w["voluntary_per_s"])
            a["idle_top_wakers_per_s"] = dict(sorted(((c, round(statistics.median(x + [0] * (len(ids) - len(x))), 1))
                                                     for c, x in wak.items()), key=lambda kv: -kv[1])[:12])
            a["idle"] = {k: spread([i.get(k) for i in ids], 2) for k in
                         ("guest_busy_pct", "guest_busy_pct_of_one_vcpu", "ctxt_per_s", "intr_per_s", "task_wakeups_per_s",
                          "qemu_cpu_pct_of_core", "qemu_vcpu_pct_of_core", "ppt_mean_w", "ppt_over_host_idle_w")}
        rs = [t["responsiveness"] for t in ts if t.get("responsiveness")]
        if rs:
            a["latency"] = {}
            for name in ("latency-idle", "latency-load"):
                xs = [r[name] for r in rs if r.get(name) and r[name].get("raw_ms")]
                raw = [x for r in xs for x in r["raw_ms"]]
                if raw:
                    rms = statistics.median([r["refresh_ms"] for r in xs])
                    st = {"p50": pct(raw, 50), "p95": pct(raw, 95), "p99": pct(raw, 99), "max": max(raw)}
                    a["latency"][name] = {"presses": len(raw), "boots": len(xs), "refresh_ms": rms,
                                          "ms": {k: round(v, 2) for k, v in st.items()},
                                          "frames": {k: round(v / rms, 2) for k, v in st.items()},
                                          "per_boot_p50_ms": [r["key_to_present_ms"]["p50"] for r in xs],
                                          "per_boot_p99_ms": [r["key_to_present_ms"]["p99"] for r in xs],
                                          "not_presented": sum(r["not_presented"] for r in xs),
                                          "key_to_client_p50_ms": spread([r["key_to_client_ms"]["p50"] for r in xs], 2),
                                          "commit_to_present_p50_ms": spread([r["commit_to_present_ms"]["p50"] for r in xs], 2)}
            for name in ("pacing-idle", "pacing-load"):
                xs = [r[name] for r in rs if r.get(name) and r[name].get("interval_ms")]
                if xs:
                    a["latency"][name] = {
                        "boots": len(xs),
                        "fps": spread([x["fps"] for x in xs], 2),
                        "interval_p50_ms": spread([x["interval_ms"]["p50"] for x in xs], 2),
                        "interval_p99_ms": spread([x["interval_ms"]["p99"] for x in xs], 2),
                        "interval_max_ms": spread([x["interval_ms"]["max"] for x in xs], 2),
                        "late_pct": spread([x["late_pct"] for x in xs], 2),
                        "missed_pct": spread([x["missed_pct"] for x in xs], 2),
                    }
        aps = [t["apps"] for t in ts if t.get("apps")]
        if aps:
            a["apps"] = {}
            for key in aps[0]:
                xs = [ap[key] for ap in aps if key in ap]
                warm = [w for x in xs for w in x["warm_settled_s"]]
                a["apps"][key] = {
                    "title": xs[0]["title"],
                    "cold_s": spread([x["cold_settled_s"] for x in xs], 2),
                    "warm_s": spread(warm, 2),
                    "cold_first_change_s": spread([x["cold_first_change_s"] for x in xs], 2),
                    "warm_first_change_s": spread([w for x in xs for w in x["warm_first_change_s"] if w is not None], 2),
                    "cold_pss_mib": spread([x["cold_pss_mib"] for x in xs]),
                    "warm_pss_mib": spread([w for x in xs for w in x["warm_pss_mib"]]),
                    "cold_other_new_pss_mib": spread([x["cold_other_new_pss_mib"] for x in xs]),
                    "timeouts": sum(x["timeouts"] for x in xs),
                }
        agg[f"{v}-{mem}"] = a
    return agg


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--runs", default=str(Path(os.environ.get("DESKBENCH_ROOT", f"{os.environ.get('TMPDIR') or '/tmp'}/aizome-perf/desktops")) / "runs"))
    ap.add_argument("--out", default=str(REPO / "docs/performance/data/desktops"))
    ap.add_argument("--keep-raw", action="store_true", help="copy compressed raw files into OUT/trials")
    args = ap.parse_args()
    runs = Path(args.runs)
    out = Path(args.out)
    (out / "trials").mkdir(parents=True, exist_ok=True)
    trials = []
    for res in sorted(runs.glob("*/*/result.json")):
        if res.parent.name.startswith("smoke"):
            continue
        run = res.parent
        try:
            s = trial_summary(run)
        except Exception as e:  # keep going; record the failure
            print("failed", run, repr(e))
            continue
        trials.append(s)
        dest = out / "trials" / f"{run.parent.name}-{run.name}"
        dest.mkdir(parents=True, exist_ok=True)
        (dest / "summary.json").write_text(json.dumps(s, indent=1))
        if args.keep_raw:
            r = json.loads(res.read_text())
            r.pop("serial_tail", None)
            with lzma.open(dest / "result.json.xz", "wt") as f:
                json.dump(r, f)
            for pattern in ("mem-*.tsv", "meminfo-*.txt", "cgmem-*.tsv", "idle-*.tsv", "idle-stat.txt", "latency-*.tsv",
                            "pacing-*.tsv", "systemd-analyze.txt", "units.txt"):
                for f in run.glob(pattern):
                    with open(f, "rb") as src, lzma.open(dest / (f.name + ".xz"), "wb") as dst:
                        shutil.copyfileobj(src, dst)
            for f in ("frames.json.xz", "journal.json.xz"):
                if (run / f).exists():
                    shutil.copy(run / f, dest / f)
    # The host, QEMU and the schedule.
    import subprocess
    try:
        qemu = subprocess.run(["qemu-system-x86_64", "--version"], capture_output=True, text=True).stdout.split("\n")[0]
    except OSError:
        qemu = None
    env = {"qemu": qemu,
           "pinned_cpus": None, "qemu_argv_example": None, "trials": []}
    for res in sorted(runs.glob("*/*/result.json")):
        if res.parent.name.startswith("smoke"):
            continue
        r = json.loads(res.read_text())
        env["pinned_cpus"] = r.get("cpus")
        argv = res.parent / "qemu-argv.json"
        if argv.exists() and env["qemu_argv_example"] is None:
            env["qemu_argv_example"] = json.loads(argv.read_text())
        env["trials"].append({"subject": res.parent.parent.name, "tag": r["tag"], "start": r["host"]["start"],
                              "end": r["host"].get("end"), "host_kernel": r["host"]["uname"],
                              "loadavg_start": r["host"]["quiet_wait"]["loadavg"],
                              "waited_for_quiet_s": r["host"]["quiet_wait"]["waited_s"],
                              "loadavg_end": r["host"].get("loadavg_end"), "guest_rtc_start": r.get("guest_rtc_start")})
    env["trials"].sort(key=lambda t: t["start"])
    (out / "environment.json").write_text(json.dumps(env, indent=1))
    agg = aggregate(trials)
    (out / "summary.json").write_text(json.dumps(agg, indent=1))
    mems = sorted({k.rsplit("-", 1)[1] for k in agg}, key=int, reverse=True)
    (out / "tables.md").write_text("\n\n".join(tables(agg, int(m)) for m in mems) + "\n")
    print(json.dumps({k: {"boot_s": v.get("boot_s"), "login_s": v.get("login_s")} for k, v in agg.items()}, indent=1))



# ---------------------------------------------------------------- tables


def fmt(s, unit="", digits=1, scale=1.0):
    if not s:
        return "–"
    med = s["median"] * scale
    lo, hi = s["min"] * scale, s["max"] * scale
    if s["n"] == 1:
        return f"{med:.{digits}f}{unit}"
    return f"{med:.{digits}f}{unit} ({lo:.{digits}f}–{hi:.{digits}f})"


def tables(agg, mem=4096):
    subjects = [v for v in ORDER if f"{v}-{mem}" in agg]
    A = {v: agg[f"{v}-{mem}"] for v in subjects}
    base = A.get("baseline", {})
    out = []
    head = "| | " + " | ".join(TITLES[v] for v in subjects) + " |\n|---|" + "---|" * len(subjects)
    def row(label, f):
        out.append(f"| {label} | " + " | ".join(f(A[v]) for v in subjects) + " |")
    out.append(f"### Boot and login ({mem} MiB; median, min–max over trials)\n")
    out.append(head)
    row("Trials", lambda a: str(len(a["trials"])))
    row("Boot: QEMU start → login screen settled (s)", lambda a: fmt(a.get("boot_s"), digits=2))
    row("&nbsp;&nbsp;kernel + initrd + userspace (systemd-analyze, s)", lambda a: " + ".join(
        f"{(a.get(k) or {}).get('median', 0):.2f}" for k in ("boot_kernel_s", "boot_initrd_s", "boot_userspace_s")))
    row("&nbsp;&nbsp;display manager started (host timeline, s)", lambda a: fmt(a.get("boot_display_manager_host_s"), digits=2))
    row("Login: Enter → desktop settled (s)", lambda a: fmt(a.get("login_s"), digits=2))
    row("&nbsp;&nbsp;Enter → first change on screen (s)", lambda a: fmt(a.get("login_first_change_s"), digits=2))
    row("&nbsp;&nbsp;Enter → PAM session opened (guest, s)", lambda a: fmt(a.get("login_enter_to_pam_s"), digits=2))
    out.append("")
    out.append(f"### Memory ({mem} MiB guest; MiB, median and range)\n")
    out.append(head)
    for name, label in (("greeter", "At the login screen"), ("desktop60", "Desktop, 60 s after login"),
                        ("desktop190", "Desktop, 190 s after login")):
        row(f"{label}: used (MemTotal − MemAvailable)", lambda a, n=name: fmt((a.get(f"mem_{n}") or {}).get("used_mib"), digits=0))
        if base.get(f"mem_{name}"):
            b = base[f"mem_{name}"]["used_mib"]["median"]
            row(f"&nbsp;&nbsp;over the no-desktop VM", lambda a, n=name: (
                f"{(a.get(f'mem_{n}') or {}).get('used_mib', {}).get('median', 0) - b:+.0f}" if a.get(f"mem_{n}") else "–"))
        row(f"&nbsp;&nbsp;swap in use", lambda a, n=name: fmt((a.get(f"mem_{n}") or {}).get("swap_used_mib"), digits=0))
    row("PSS at 190 s: system services", lambda a: fmt((a.get("mem_desktop190") or {}).get("pss_mib", {}).get("system"), digits=0))
    row("PSS at 190 s: login screen (still running)", lambda a: fmt((a.get("mem_desktop190") or {}).get("pss_mib", {}).get("greeter"), digits=0))
    row("PSS at 190 s: the user's session", lambda a: fmt((a.get("mem_desktop190") or {}).get("pss_mib", {}).get("session"), digits=0))
    out.append("")
    out.append(f"### Idle cost over 120 s at the desktop ({mem} MiB)\n")
    out.append(head)
    row("Guest CPU busy (% of one vCPU)", lambda a: fmt((a.get("idle") or {}).get("guest_busy_pct_of_one_vcpu"), digits=2))
    row("Context switches / s", lambda a: fmt((a.get("idle") or {}).get("ctxt_per_s"), digits=0))
    row("Thread wakeups / s (voluntary switches)", lambda a: fmt((a.get("idle") or {}).get("task_wakeups_per_s"), digits=0))
    row("Interrupts / s", lambda a: fmt((a.get("idle") or {}).get("intr_per_s"), digits=0))
    row("Host: QEMU CPU (% of one core)", lambda a: fmt((a.get("idle") or {}).get("qemu_cpu_pct_of_core"), digits=2))
    row("Host: PPT over host idle (W)", lambda a: fmt((a.get("idle") or {}).get("ppt_over_host_idle_w"), digits=2))
    out.append("")
    if any(A[v].get("latency") for v in subjects):
        ds = [v for v in subjects if A[v].get("latency")]
        out.append(f"### Input to screen and frame pacing ({mem} MiB)\n")
        out.append("| | " + " | ".join(TITLES[v] for v in ds) + " |\n|---|" + "---|" * len(ds))
        def lrow(label, f):
            out.append(f"| {label} | " + " | ".join(f(A[v]["latency"]) for v in ds) + " |")
        lrow("Refresh the compositor reports (ms)", lambda l: f"{l.get('latency-idle', {}).get('refresh_ms', 0):.2f}")
        for cond in ("idle", "load"):
            k = f"latency-{cond}"
            lrow(f"Key → presented, {cond}: p50 / p95 / p99 / max (ms)", lambda l, k=k: " / ".join(
                f"{l[k]['ms'][p]:.1f}" for p in ("p50", "p95", "p99", "max")) if l.get(k) else "–")
            lrow(f"&nbsp;&nbsp;in frames: p50 / p99", lambda l, k=k: " / ".join(
                f"{l[k]['frames'][p]:.2f}" for p in ("p50", "p99")) if l.get(k) else "–")
            lrow(f"&nbsp;&nbsp;presses (boots)", lambda l, k=k: f"{l[k]['presses']} ({l[k]['boots']})" if l.get(k) else "–")
            k2 = f"pacing-{cond}"
            lrow(f"Continuous frames, {cond}: fps", lambda l, k=k2: fmt(l.get(k, {}).get("fps"), digits=1))
            lrow(f"&nbsp;&nbsp;interval p99 / max (ms)", lambda l, k=k2: (
                f"{l[k]['interval_p99_ms']['median']:.1f} / {l[k]['interval_max_ms']['median']:.1f}") if l.get(k) else "–")
            lrow(f"&nbsp;&nbsp;refreshes missed (%)", lambda l, k=k2: fmt(l.get(k, {}).get("missed_pct"), digits=2))
        out.append("")
    if any(A[v].get("apps") for v in subjects):
        out.append(f"### App start-up ({mem} MiB): launch → window settled (s); first change on screen (s); PSS (MiB)\n")
        out.append("| Desktop | App | Settled, cold | Settled, warm | First change, cold | First change, warm | PSS cold | PSS warm | Other new processes, cold (MiB) |\n|---|---|---|---|---|---|---|---|---|")
        for v in subjects:
            for key, x in (A[v].get("apps") or {}).items():
                out.append(f"| {TITLES[v]} | {x['title']} ({key}) | {fmt(x['cold_s'], digits=2)} | {fmt(x['warm_s'], digits=2)} | "
                           f"{fmt(x['cold_first_change_s'], digits=2)} | {fmt(x['warm_first_change_s'], digits=2)} | "
                           f"{fmt(x['cold_pss_mib'], digits=0)} | {fmt(x['warm_pss_mib'], digits=0)} | {fmt(x['cold_other_new_pss_mib'], digits=0)} |")
        out.append("")
    return "\n".join(out)

if __name__ == "__main__":
    main()
