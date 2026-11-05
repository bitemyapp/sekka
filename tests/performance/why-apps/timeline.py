#!/usr/bin/env python3
"""Phases of each program's start, from stamps.so's lines (probe.py's
`stamps` set): medians over the runs, in ms from the harness's fork (t0).

    timeline.py RUNDIR [NAME...]                medians, in Markdown
    timeline.py RUNDIR NAME --run REP            one run's stamps, in order

Milestones: the first exec'd binary's constructor (after its libraries are
mapped and relocated), the real program's (after a Nix wrapper's exec),
main(), the xdg toplevel's creation, the first commit of a buffer to it,
and sway's window `new` event. Spans: dlopen (outermost calls only, by
kind of library), Vulkan and EGL set-up, fontconfig, blocking D-Bus calls,
Wayland roundtrips.
"""
import collections
import json
import os
import re
import statistics
import sys


def load(path):
    out = []
    for line in open(path):
        parts = line.rstrip("\n").split(" ", 5)
        if len(parts) < 5:
            continue
        t, pid, tid, ev, dur = int(parts[0]), int(parts[1]), int(parts[2]), parts[3], int(parts[4])
        out.append({"t": t, "pid": pid, "tid": tid, "ev": ev, "dur": dur, "detail": parts[5] if len(parts) > 5 else ""})
    return out


def dlopen_kind(path):
    p = path
    if re.search(r"libvulkan_|libGLX_nvidia|libnvidia|libEGL_|libgallium|libVkLayer|libGL|libgbm", p):
        return "GPU drivers and Vulkan ICDs"
    if "/qt-6/plugins/" in p or "/qml/" in p:
        return "Qt plugins and QML modules"
    if "/gio/modules/" in p:
        return "GIO modules"
    if "gtk-4.0" in p or "gtk-3.0" in p:
        return "GTK modules"
    if "fontconfig" in p:
        return "fontconfig"
    return "other"


def outermost(spans):
    """Spans (t, dur) of one thread, without those inside another."""
    spans = sorted(spans, key=lambda s: (s["t"], -s["dur"]))
    out, end = [], -1
    for s in spans:
        if s["t"] >= end:
            out.append(s)
            end = s["t"] + s["dur"]
    return out


def analyse(lines, run):
    t0 = run["t0_mono_ns"]
    ms = lambda t: (t - t0) / 1e6
    res = {}
    ctors = [l for l in lines if l["ev"] == "ctor"]
    mains = [l for l in lines if l["ev"] == "main"]
    if ctors:
        res["first exec: libraries mapped and relocated (first ctor)"] = ms(ctors[0]["t"])
        res["program's ctor (after a Nix wrapper's exec)"] = ms(ctors[-1]["t"]) if len(ctors) > 1 else ms(ctors[0]["t"])
    if mains:
        res["main()"] = ms(mains[-1]["t"])
    top = [l for l in lines if l["ev"] == "xdg_toplevel"]
    if top:
        res["xdg toplevel created"] = ms(top[0]["t"])
    fr = [l for l in lines if l["ev"] == "commit-buffer" and "toplevel" in l["detail"]]
    if fr:
        res["first buffer committed to the toplevel"] = ms(fr[0]["t"])
    if run.get("t_map_mono_ns"):
        res["sway's window new event (first frame)"] = ms(run["t_map_mono_ns"])
    end = fr[0]["t"] if fr else run.get("t_map_mono_ns", 1 << 62)
    main_tid = ctors[-1]["tid"] if ctors else None
    spans = collections.Counter()
    # dlopen, outermost per thread, before the first frame.
    by_tid = collections.defaultdict(list)
    for l in lines:
        if l["ev"] == "dlopen" and l["t"] < end:
            by_tid[l["tid"]].append(l)
    for tid, ls in by_tid.items():
        for s in outermost(ls):
            spans[f"dlopen: {dlopen_kind(s['detail'])}"] += s["dur"] / 1e6
            spans["dlopen: all (outermost)"] += s["dur"] / 1e6
    groups = {
        "Vulkan: instance, ICDs, devices": ("vkEnumerateInstanceExtensionProperties", "vkCreateInstance",
                                            "vkEnumeratePhysicalDevices", "vkCreateDevice"),
        "Vulkan: pipelines": ("vkCreateGraphicsPipelines",),
        "EGL: display, initialize, context": ("eglGetDisplay", "eglGetPlatformDisplay", "eglGetPlatformDisplayEXT",
                                              "eglInitialize", "eglCreateContext"),
        "fontconfig (any thread)": ("FcInit", "FcInitLoadConfigAndFonts", "FcConfigGetCurrent", "FcFontList",
                                    "FcFontSort", "FcFontMatch"),
        "D-Bus: blocking calls": ("g_bus_get_sync", "g_dbus_connection_new_for_address_sync",
                                  "g_dbus_connection_call_sync", "g_dbus_proxy_new_sync", "dbus_bus_get_private",
                                  "dbus_connection_open_private", "dbus_call_block"),
        "Wayland: connect and roundtrips": ("wl_display_connect", "wl_display_roundtrip"),
        "xkbcommon": ("xkb_context_new", "xkb_keymap_new", "xkb_compose_table"),
    }
    for g, evs in groups.items():
        ls = [l for l in lines if l["ev"] in evs and l["t"] < end and (g.startswith("fontconfig") or l["tid"] == main_tid)]
        # Nested calls (vkCreateInstance inside a loader call) once.
        tot = 0.0
        for tid in {l["tid"] for l in ls}:
            tot += sum(s["dur"] for s in outermost([l for l in ls if l["tid"] == tid])) / 1e6
        spans[g] += tot
    polls = [l for l in lines if l["ev"] == "poll" and l["tid"] == main_tid and l["t"] < end]
    spans["main thread blocked in poll (>= 0.2 ms each)"] = sum(min(l["dur"], end - l["t"]) for l in polls) / 1e6
    return res, spans


def main():
    args = sys.argv[1:]
    rundir = args.pop(0)
    one = None
    if "--run" in args:
        i = args.index("--run")
        one = args[i + 1]
        del args[i:i + 2]
    runs = [json.loads(l) for l in open(f"{rundir}/runs.jsonl")]
    runs = [r for r in runs if not r.get("meta") and not r.get("warmup")]
    names = args or list(dict.fromkeys(r["name"] for r in runs))
    if one is not None:
        r = [r for r in runs if r["name"] == names[0] and str(r["rep"]) == one][0]
        t0 = r["t0_mono_ns"]
        for l in sorted(load(f"{rundir}/{names[0]}-{one}.stamps"), key=lambda l: l["t"]):
            if l["ev"] == "poll" and l["dur"] < 500000:
                continue
            d = re.sub(r"/nix/store/[a-z0-9]{32}-", "", l["detail"])
            print(f"{(l['t'] - t0) / 1e6:8.2f} {l['dur'] / 1e6:8.2f}  tid+{l['tid'] - l['pid']:<4} {l['ev']:<32} {d}")
        if r.get("t_map_mono_ns"):
            print(f"{(r['t_map_mono_ns'] - t0) / 1e6:8.2f}           sway: window new")
        return
    print("Milestones, ms from the fork (median over runs):\n")
    table = collections.OrderedDict()
    spans_t = collections.OrderedDict()
    for name in names:
        rs = [r for r in runs if r["name"] == name]
        ms_all = collections.defaultdict(list)
        sp_all = collections.defaultdict(list)
        for r in rs:
            path = f"{rundir}/{name}-{r['rep']}.stamps"
            if not os.path.exists(path):
                continue
            res, spans = analyse(load(path), r)
            for k, v in res.items():
                ms_all[k].append(v)
            for k in set(spans) | set(sp_all):
                sp_all[k].append(spans.get(k, 0.0))
        table[name] = {k: statistics.median(v) for k, v in ms_all.items()}
        spans_t[name] = {k: statistics.median(v) for k, v in sp_all.items()}
    keys = list(dict.fromkeys(k for v in table.values() for k in v))
    print("| Milestone | " + " | ".join(table) + " |")
    print("|---|" + "---:|" * len(table))
    for k in keys:
        print(f"| {k} | " + " | ".join(f"{table[n].get(k, float('nan')):.1f}" for n in table) + " |")
    print("\nTime inside each kind of call before the first frame, ms (median; main thread unless noted):\n")
    keys = sorted(dict.fromkeys(k for v in spans_t.values() for k in v))
    print("| Span | " + " | ".join(spans_t) + " |")
    print("|---|" + "---:|" * len(spans_t))
    for k in keys:
        print(f"| {k} | " + " | ".join(f"{spans_t[n].get(k, 0.0):.1f}" for n in spans_t) + " |")


if __name__ == "__main__":
    main()
