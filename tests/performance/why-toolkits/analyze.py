#!/usr/bin/env python3
"""Summaries of the why-toolkits runs (docs/performance/why-toolkits.md).

    analyze.py RUNS OUT.json [OUT.md]

RUNS holds SCENARIO/SUBJECT-rN/result.json (+ trace.log, ppt.tsv) from
run_sway.py or run_native.py. Each trial is reduced to a row of figures;
trials are pooled per scenario and subject (medians across trials; latency
percentiles across every keystroke)."""
import json
import os
import statistics
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import tk  # noqa: E402
import trace as T  # noqa: E402


def ms(a, b):
    return (a - b) / 1e6 if a is not None and b is not None else None


def rate(d):
    return d["cpu_ms"] / d["seconds"] if d and d["seconds"] else None


def power_delta(trial, w0, w1, b0, b1):
    """Mean socket power over [w0, w1] minus over [b0, b1] (W), and both."""
    e = tk.ppt_energy(f"{trial}/ppt.tsv", w0, w1)
    b = tk.ppt_energy(f"{trial}/ppt.tsv", b0, b1)
    if not e or not b:
        return None
    return dict(window_w=e["mean_w"], base_w=b["mean_w"], delta_w=e["mean_w"] - b["mean_w"],
                seconds=(w1 - w0) / 1e9)


def main_frames(fr, t0, t1):
    """Content commits in the window, on the surface that commits most
    (the window's own; subsurfaces within 2 ms count as one frame)."""
    w = [f for f in fr if t0 <= f["t"] <= t1]
    groups = []
    for f in w:
        if groups and f["t"] - groups[-1][-1]["t"] < 2_000_000 and f["client"] == groups[-1][-1]["client"]:
            groups[-1].append(f)
        else:
            groups.append([f])
    return groups


def trial_latency(r):
    lat = [x for x in r.get("latency_ms", [])]
    return dict(latency=lat, latency_full=r.get("latency_full_ms", []), missed=r.get("missed", 0))


def trial_keys(d, r):
    ev = T.load(f"{d}/trace.log")
    ty = r["typing"]
    presses = [k for k in T.keys(ev) if ty["t0"] <= k[0] <= ty["t1"]]
    rs = T.responses(ev, presses)
    inj = [x for x in r.get("injects", []) if x]
    # Each injected key is matched to the next press delivered after it.
    deliver = []
    j = 0
    for t in inj:
        while j < len(presses) and presses[j][0] < t:
            j += 1
        if j < len(presses) and presses[j][0] - t < 200e6:
            deliver.append(ms(presses[j][0], t))
    keys = ty["keys"]
    idle = r.get("idle", {})
    app_idle = rate(idle.get("app")) or 0
    comp_idle = rate(idle.get("comp")) or 0
    secs = ty["app"]["seconds"]
    out = dict(
        n=len(rs),
        deliver=deliver,
        key_commit=[ms(x["commit_t"], x["key_t"]) for x in rs],
        commit_done=[ms(x["done_t"], x["commit_t"]) for x in rs],
        commit_presented=[ms(x["presented_t"], x["commit_t"]) for x in rs],
        key_done=[ms(x["done_t"], x["key_t"]) for x in rs],
        key_presented=[ms(x["presented_t"], x["key_t"]) for x in rs],
        area_px=[x["area_px"] for x in rs],
        frames_per_key=[x["frames"] for x in rs],
        kinds="".join(sorted(set(x["kinds"] for x in rs))),
        app_cpu_ms_per_key=(ty["app"]["cpu_ms"] - app_idle * secs) / keys,
        comp_cpu_ms_per_key=(ty["comp"]["cpu_ms"] - comp_idle * secs) / keys,
        app_gpu_ms_per_key=ty["app"]["gpu_ms"] / keys,
        comp_gpu_ms_per_key=ty["comp"]["gpu_ms"] / keys,
        app_wakeups_per_key=ty["app"]["vol"] / keys,
        comp_wakeups_per_key=ty["comp"]["vol"] / keys,
        app_idle_cpu_ms_per_s=app_idle, comp_idle_cpu_ms_per_s=comp_idle,
        app_idle_wakeups_per_s=idle.get("app", {}).get("vol", 0) / idle["app"]["seconds"] if idle.get("app") else None,
        threads={k: v / keys for k, v in ty["app"]["threads"].items()},
    )
    iw = r.get("idle_window")
    if iw:
        p = power_delta(d, ty["t0"], ty["t1"], iw[0], iw[1])
        if p:
            out["power"] = p
            out["energy_mj_per_key"] = p["delta_w"] * p["seconds"] / keys * 1000
    return out


def trial_window(d, r, hz):
    """Scroll, resize and idle: frames in the measured window and their cost."""
    ws = r["window_stats"]
    t0, t1 = ws["t0"], ws["t1"]
    secs = (t1 - t0) / 1e9
    out = dict(seconds=secs)
    if os.path.exists(f"{d}/trace.log"):
        ev = T.load(f"{d}/trace.log")
        fr = T.frames(ev)
        groups = main_frames(fr, t0, t1)
        n = len(groups)
        out["frames"] = n
        out["fps"] = n / secs
        times = [g[-1]["t"] for g in groups]
        iv = [(b - a) / 1e6 for a, b in zip(times, times[1:])]
        out["interval_ms"] = T.summary(iv)
        period = 1000 / hz
        out["long_intervals"] = sum(1 for x in iv if x > 1.5 * period)
        dones = sorted(g_done for g in groups for g_done in [min([f["done"] for f in g if f["done"]], default=None)] if g_done)
        div = [(b - a) / 1e6 for a, b in zip(dones, dones[1:])]
        out["done_interval_ms"] = T.summary(div)
        # The compositor's frame callback to the program's next frame: for a
        # program that draws on frame callbacks, the time it takes to make a
        # frame (from the callback's arrival to the commit).
        d2c, j = [], 0
        for k, td in enumerate(dones):
            nxt = dones[k + 1] if k + 1 < len(dones) else float("inf")
            while j < len(times) and times[j] <= td:
                j += 1
            if j < len(times) and times[j] < nxt:
                d2c.append((times[j] - td) / 1e6)
        out["done_to_commit_ms"] = T.summary(d2c)
        areas = [sum(f["area_px"] for f in g) for g in groups]
        out["area_px_per_frame"] = statistics.median(areas) if areas else None
        kinds = "".join(sorted(set(f["kind"] for g in groups for f in g)))
        out["kinds"] = kinds
        shm = [sum(f["area_px"] for f in g if f["kind"] == "s") for g in groups]
        out["shm_bytes_per_frame"] = statistics.median(shm) * 4 if shm else 0
        out["buffer_px"] = statistics.median([max(f["w"] * f["h"] for f in g) for g in groups]) if groups else None
        if r["scenario"] == "resize":
            out.update(resize_latency(ev, t0, t1))
    else:
        n = None
    app, comp = ws["app"], ws["comp"]
    base = r.get("comp_baseline")
    comp_base = rate(base) if base else 0
    out.update(app_cpu_ms_per_s=rate(app), comp_cpu_ms_per_s=rate(comp), comp_base_cpu_ms_per_s=comp_base,
               app_gpu_ms_per_s=app["gpu_ms"] / secs, comp_gpu_ms_per_s=comp["gpu_ms"] / secs,
               app_wakeups_per_s=app["vol"] / secs, threads={k: v / secs for k, v in app["threads"].items()})
    if n:
        out.update(app_cpu_ms_per_frame=app["cpu_ms"] / n, comp_cpu_ms_per_frame=(comp["cpu_ms"] - comp_base * secs) / n,
                   app_gpu_ms_per_frame=app["gpu_ms"] / n, comp_gpu_ms_per_frame=comp["gpu_ms"] / n)
    bw = r.get("baseline_window")
    if bw:
        p = power_delta(d, t0, t1, bw[0], bw[1])
        if p:
            out["power"] = p
            if n:
                out["energy_mj_per_frame"] = p["delta_w"] * secs / n * 1000
    return out


def resize_latency(ev, t0, t1):
    """Each new size the compositor configures (sway configures each size
    twice: the size, then again after the commit), to the first commit of a
    buffer that wide; and how often the committed width changes."""
    lat = []
    last_conf = None
    pending = None
    last_w = None
    changes = 0
    for t, c, e, f in ev:
        if not (t0 <= t <= t1):
            continue
        if e == "tconf" and int(f[0]) > 0:
            size = (int(f[0]), int(f[1]))
            if size != last_conf:
                pending = (t, size[0])
                last_conf = size
        elif e == "attach" and f[2] in "sd":
            w = int(f[3])
            if last_w is not None and w != last_w:
                changes += 1
            last_w = w
            if pending and abs(w - pending[1]) <= 2:
                lat.append((t - pending[0]) / 1e6)
                pending = None
    secs = (t1 - t0) / 1e9
    return dict(configure_to_commit_ms=T.summary(lat), sizes_per_s=changes / secs)


def pool(values):
    v = [x for x in values if x is not None]
    return T.summary(v)


def main():
    runs, out_json = sys.argv[1], sys.argv[2]
    rows = {}
    for sc in sorted(os.listdir(runs)):
        sd = f"{runs}/{sc}"
        if not os.path.isdir(sd):
            continue
        for trial in sorted(os.listdir(sd)):
            d = f"{sd}/{trial}"
            if not os.path.exists(f"{d}/result.json"):
                continue
            r = json.load(open(f"{d}/result.json"))
            subj = trial.rsplit("-r", 1)[0]
            hz = r.get("hz", 60)
            try:
                if r["scenario"] == "latency":
                    row = trial_latency(r)
                elif r["scenario"] == "keys":
                    row = trial_keys(d, r)
                else:
                    row = trial_window(d, r, hz)
            except Exception as e:  # noqa: BLE001
                row = dict(error=repr(e))
            row["trial"] = trial
            row["hz"] = hz
            row["load_before"] = r.get("load_before")
            rows.setdefault(sc, {}).setdefault(subj, []).append(row)
    summary = {}
    for sc, subs in rows.items():
        for subj, trials in subs.items():
            s = dict(trials=len(trials), errors=[t["error"] for t in trials if "error" in t])
            good = [t for t in trials if "error" not in t]
            if not good:
                summary.setdefault(sc, {})[subj] = s
                continue
            hz = good[0]["hz"]
            if "latency" in good[0]:
                lat = [x for t in good for x in t["latency"]]
                s["latency_ms"] = pool(lat)
                s["latency_frames"] = pool([x * hz / 1000 for x in lat])
                full = [x for t in good for x in t.get("latency_full", [])]
                s["latency_full_ms"] = pool(full)
                s["latency_full_frames"] = pool([x * hz / 1000 for x in full])
                s["missed"] = sum(t["missed"] for t in good)
                s["per_trial_p50"] = [T.pct(t["latency"], 0.5) for t in good if t["latency"]]
            elif "key_commit" in good[0]:
                for k in ("deliver", "key_commit", "commit_done", "commit_presented", "key_done", "key_presented",
                          "area_px", "frames_per_key"):
                    s[k] = pool([x for t in good for x in t[k]])
                s["kinds"] = "".join(sorted(set("".join(t["kinds"] for t in good))))
                for k in ("app_cpu_ms_per_key", "comp_cpu_ms_per_key", "app_gpu_ms_per_key", "comp_gpu_ms_per_key",
                          "app_wakeups_per_key", "comp_wakeups_per_key", "app_idle_cpu_ms_per_s",
                          "comp_idle_cpu_ms_per_s", "app_idle_wakeups_per_s", "energy_mj_per_key"):
                    v = [t.get(k) for t in good if t.get(k) is not None]
                    s[k] = dict(median=statistics.median(v), min=min(v), max=max(v)) if v else None
                s["threads_ms_per_key"] = {k: statistics.median([t["threads"].get(k, 0) for t in good])
                                           for k in good[0]["threads"]}
            else:
                for k in ("fps", "frames", "long_intervals", "area_px_per_frame", "shm_bytes_per_frame", "buffer_px",
                          "app_cpu_ms_per_s", "comp_cpu_ms_per_s", "app_gpu_ms_per_s", "comp_gpu_ms_per_s",
                          "app_cpu_ms_per_frame", "comp_cpu_ms_per_frame", "app_gpu_ms_per_frame",
                          "comp_gpu_ms_per_frame", "app_wakeups_per_s", "energy_mj_per_frame", "comp_base_cpu_ms_per_s",
                          "sizes_per_s"):
                    v = [t.get(k) for t in good if t.get(k) is not None]
                    s[k] = dict(median=statistics.median(v), min=min(v), max=max(v)) if v else None
                s["kinds"] = "".join(sorted(set("".join(t.get("kinds", "") for t in good))))
                s["interval_p99_ms"] = statistics.median([t["interval_ms"]["p99"] for t in good if t.get("interval_ms")]) \
                    if any(t.get("interval_ms") for t in good) else None
                d2c = [t["done_to_commit_ms"] for t in good if t.get("done_to_commit_ms")]
                if d2c:
                    s["done_to_commit_p50_ms"] = statistics.median([x["p50"] for x in d2c])
                    s["done_to_commit_p90_ms"] = statistics.median([x["p90"] for x in d2c])
                c2c = [t.get("configure_to_commit_ms") for t in good if t.get("configure_to_commit_ms")]
                if c2c:
                    s["configure_to_commit_p50_ms"] = statistics.median([c["p50"] for c in c2c])
                    s["configure_to_commit_p90_ms"] = statistics.median([c["p90"] for c in c2c])
                pw = [t["power"]["delta_w"] for t in good if t.get("power")]
                s["power_delta_w"] = dict(median=statistics.median(pw), min=min(pw), max=max(pw)) if pw else None
                s["threads_ms_per_s"] = {k: statistics.median([t["threads"].get(k, 0) for t in good])
                                         for k in good[0].get("threads", {})}
            s["load_before"] = [t["load_before"][0] if t.get("load_before") else None for t in good]
            summary.setdefault(sc, {})[subj] = s
    with open(out_json, "w") as f:
        json.dump(dict(summary=summary, trials=rows), f, indent=1, default=str)


if __name__ == "__main__":
    main()
