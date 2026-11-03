#!/usr/bin/env python3
"""Reading the tracing proxy's log (wltrace.c) for docs/performance/why-toolkits.md.

A program's frames are its commits that carry content (a buffer attached or
damage since the surface's last commit). For each, the log gives the damage
(in buffer pixels), the kind of buffer (s: wl_shm, d: linux-dmabuf, p: single
pixel), the frame callback the program asked for with it and when the
compositor answered it ("done"), and wp_presentation feedback where the
program asked for it. Subsurfaces: a synchronized subsurface's commit only
takes effect at its parent's commit; both are counted as one frame when they
come within 2 ms of each other."""
import statistics


def load(path):
    out = []
    with open(path) as f:
        for line in f:
            p = line.split()
            if len(p) >= 3 and p[0].isdigit():
                out.append((int(p[0]), int(p[1]), p[2], p[3:]))
    return out


def union_area(boxes):
    """The area the rectangles (x0, y0, x1, y1) cover together: damage
    rectangles overlap (the kit damages a moved region and the strip it
    uncovers)."""
    if len(boxes) <= 1:
        return sum((b[2] - b[0]) * (b[3] - b[1]) for b in boxes)
    ys = sorted(set(y for b in boxes for y in (b[1], b[3])))
    area = 0
    for ya, yb in zip(ys, ys[1:]):
        spans = sorted((b[0], b[2]) for b in boxes if b[1] <= ya and b[3] >= yb)
        covered, end = 0, None
        for x0, x1 in spans:
            if end is None or x0 > end:
                covered += x1 - x0
                end = x1
            elif x1 > end:
                covered += x1 - end
                end = x1
        area += covered * (yb - ya)
    return area


def frames(events):
    """Every content commit: dict(t, client, surface, damage_px, area_px,
    kind, w, h, done, presented)."""
    pend = {}       # (client, surface) -> dict(damage, attach)
    scale = {}
    out = []
    by_cb = {}      # (client, callback) -> frame
    by_fb = {}      # (client, feedback) -> frame
    cb_pending = {}  # (client, surface) -> [callback ids requested since commit]
    fb_pending = {}
    for t, c, ev, f in events:
        if ev == "attach":
            s = int(f[0])
            p = pend.setdefault((c, s), dict(rects=[], attach=None))
            p["attach"] = (f[2], int(f[3]), int(f[4]))
        elif ev in ("damage", "dbuf"):
            s = int(f[0])
            x, y, w, h = (int(v) for v in f[1:5])
            k = scale.get((c, s), 1) if ev == "damage" else 1
            p = pend.setdefault((c, s), dict(rects=[], attach=None))
            p["rects"].append((x * k, y * k, w * k, h * k))
        elif ev == "scale":
            scale[(c, int(f[0]))] = int(f[1])
        elif ev == "frame":
            cb_pending.setdefault((c, int(f[0])), []).append(int(f[1]))
        elif ev == "fb":
            fb_pending.setdefault((c, int(f[0])), []).append(int(f[1]))
        elif ev == "commit":
            s = int(f[0])
            p = pend.pop((c, s), None)
            cbs = cb_pending.pop((c, s), [])
            fbs = fb_pending.pop((c, s), [])
            if not p:
                continue
            kind, w, h = p["attach"] if p["attach"] else ("-", 0, 0)
            boxes = []
            for (x, y, rw, rh) in p["rects"]:
                # Clip to the buffer (toolkits damage INT32_MAX x INT32_MAX
                # for "everything").
                bw, bh = (w, h) if w and h else (1 << 16, 1 << 16)
                x0, y0 = max(0, x), max(0, y)
                x1, y1 = min(bw, x + rw if rw < 2**30 else bw), min(bh, y + rh if rh < 2**30 else bh)
                if x1 > x0 and y1 > y0:
                    boxes.append((x0, y0, x1, y1))
            area = union_area(boxes)
            fr = dict(t=t, client=c, surface=s, rects=len(p["rects"]), area_px=area, kind=kind, w=w, h=h,
                      attached=p["attach"] is not None, done=None, presented=None)
            out.append(fr)
            for cb in cbs:
                by_cb[(c, cb)] = fr
            for fb in fbs:
                by_fb[(c, fb)] = fr
        elif ev == "done":
            fr = by_cb.pop((c, int(f[1])), None)
            if fr and fr["done"] is None:
                fr["done"] = t
        elif ev == "presented":
            fr = by_fb.pop((c, int(f[1])), None)
            if fr and fr["presented"] is None:
                fr["presented"] = int(f[2])
    return out


def keys(events, state="1"):
    """Key presses delivered: (t, client, key)."""
    return [(t, c, int(f[1])) for t, c, ev, f in events if ev == "key" and f[2] == state]


def responses(events, presses=None):
    """For each key press: the first content commit after it (any surface of
    any connection), its frame callback's answer and presentation, and the
    commits until the next press."""
    fr = frames(events)
    presses = presses if presses is not None else keys(events)
    out = []
    j = 0
    for i, (t, c, k) in enumerate(presses):
        t_next = presses[i + 1][0] if i + 1 < len(presses) else float("inf")
        while j < len(fr) and fr[j]["t"] <= t:
            j += 1
        mine = [x for x in fr[j:] if x["t"] < t_next]
        # Count commits within 2 ms of each other (subsurfaces) as one.
        groups = []
        for x in mine:
            if groups and x["t"] - groups[-1][-1]["t"] < 2_000_000:
                groups[-1].append(x)
            else:
                groups.append([x])
        first = groups[0] if groups else []
        done = [x["done"] for x in first if x["done"]]
        pres = [x["presented"] for x in first if x["presented"]]
        out.append(dict(
            key_t=t, key=k,
            commit_t=first[-1]["t"] if first else None,
            done_t=min(done) if done else None,
            presented_t=min(pres) if pres else None,
            area_px=sum(x["area_px"] for x in first),
            kinds="".join(sorted(set(x["kind"] for x in first))),
            surfaces=len(first),
            frames=len(groups),
            area_all_px=sum(x["area_px"] for x in mine),
        ))
    return out


def pct(v, q):
    v = sorted(v)
    if not v:
        return None
    k = (len(v) - 1) * q
    lo = int(k)
    hi = min(lo + 1, len(v) - 1)
    return v[lo] + (v[hi] - v[lo]) * (k - lo)


def summary(v):
    v = [x for x in v if x is not None]
    if not v:
        return None
    return dict(n=len(v), p50=pct(v, 0.5), p90=pct(v, 0.9), p99=pct(v, 0.99), mean=statistics.fmean(v),
                min=min(v), max=max(v))
