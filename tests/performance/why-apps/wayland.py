#!/usr/bin/env python3
"""Wayland milestones of each run of probe.py's `wldebug` set
(WAYLAND_DEBUG=1), in ms from the harness's fork, beside sway's window
event.

    wayland.py RUNDIR

libwayland prints each message's CLOCK_REALTIME time of day in UTC
([HH:MM:SS.uuuuuu]); wayland-rs (the kit) prints microseconds since the
epoch modulo 2^32 as [ms.uuu]. Both are put on the run's clock with the
CLOCK_REALTIME stamp probe.py takes at the fork.
"""
import datetime
import json
import re
import sys


def events(path, t0_real_ns):
    t0us = t0_real_ns // 1000
    d = datetime.datetime.fromtimestamp(t0us / 1e6, datetime.timezone.utc)
    t0_tod = d.hour * 3600 + d.minute * 60 + d.second + d.microsecond / 1e6
    out = []
    for line in open(path, errors="replace"):
        m = re.match(r"^\[(\d\d):(\d\d):(\d\d)\.(\d{6})\]\s*(.*)$", line)
        if m:
            tod = int(m.group(1)) * 3600 + int(m.group(2)) * 60 + int(m.group(3)) + int(m.group(4)) / 1e6
            out.append(((tod - t0_tod) * 1000, m.group(5)))
            continue
        m = re.match(r"^\[\s*(\d+)\.(\d{3})\]\s*(.*)$", line)
        if m:
            us = int(m.group(1)) * 1000 + int(m.group(2))
            out.append((((us - t0us % (1 << 32)) % (1 << 32)) / 1000, m.group(3)))
    return out


def main():
    rundir = sys.argv[1]
    runs = [json.loads(l) for l in open(f"{rundir}/runs.jsonl")]
    runs = [r for r in runs if not r.get("meta") and not r.get("warmup")]
    print("| Program | rep | connected (get_registry) | xdg toplevel | first buffer attached | sway's window event | event less attach |")
    print("|---|---:|---:|---:|---:|---:|---:|")
    for r in sorted(runs, key=lambda r: (r["name"], r["rep"])):
        ev = events(f"{rundir}/{r['name']}-{r['rep']}.stderr", r["t0_real_ns"])
        first = lambda rx: next((t for t, e in ev if re.search(rx, e)), None)
        reg = first(r"-> wl_display[#@]1\.get_registry")
        top = first(r"get_toplevel")
        att = first(r"-> wl_surface[#@]\d+\.attach\((new id )?wl_buffer")
        ff = r.get("first_frame_ms")
        f = lambda x: "–" if x is None else f"{x:.1f}"
        print(f"| {r['name']} | {r['rep']} | {f(reg)} | {f(top)} | {f(att)} | {f(ff)} | "
              f"{f(ff - att) if ff is not None and att is not None else '–'} |")


if __name__ == "__main__":
    main()
