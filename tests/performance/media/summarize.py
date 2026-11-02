#!/usr/bin/env python3
"""Summarise bench.py results: per (compositor, mode, clip, subject), the
median and range over trials of power, its delta over the idle baseline of
the same batch and round, CPU, switches, iGPU use, memory and frames.

    summarize.py OUT_DIR RESULTS_DIR...

Writes OUT_DIR/summary.csv (one row per group), OUT_DIR/trials.csv (one row
per trial, the numbers the groups are made of) and OUT_DIR/summary.md
(tables by mode).
"""

import csv
import json
import os
import statistics
import sys

ORDER = ["baseline", "utsushi", "mpv-vaapi-dmabuf", "mpv-vaapi-gpunext", "mpv-default", "mpv-sekka", "vlc-default",
         "showtime", "haruna"]
CLIP_ORDER = ["tos-1080p24-h264", "tos-1080p24-hevc10", "tos-1080p24-vp9", "tos-1080p24-av1",
              "bbb-2160p30-hevc10", "bbb-2160p60-av1", "bbb-music.mp3", "bbb-music.opus"]


def clip_name(spec):
    c = spec.get("clip")
    if not c:
        return "-"
    base, ext = os.path.splitext(c)
    # The music is the same piece in two formats: keep them apart.
    return c if ext in (".mp3", ".opus", ".flac") else base


def clip_file(spec):
    return spec.get("clip") or "-"


def num(x):
    return x if isinstance(x, (int, float)) else None


def frames(rec):
    """Frames shown and dropped over the window, from the player's own counters."""
    a, b = rec.get("player_stats_before") or {}, rec.get("player_stats_after") or {}
    s = rec["spec"]["subject"]
    out = {}
    if s.startswith("mpv"):
        for k in ("frame-drop-count", "decoder-frame-drop-count", "vo-delayed-frame-count"):
            if num(a.get(k)) is not None and num(b.get(k)) is not None:
                out[k] = b[k] - a[k]
        out["dropped"] = (out.get("frame-drop-count") or 0) + (out.get("decoder-frame-drop-count") or 0)
        out["late"] = out.get("vo-delayed-frame-count")
        out["decoder"] = b.get("hwdec-current")
        out["vo"] = b.get("current-vo")
        out["gpu_context"] = b.get("current-gpu-context")
        out["format"] = b.get("video-params/hw-pixelformat") or b.get("video-params/pixelformat")
        if num(a.get("playback-time")) is not None and num(b.get("playback-time")) is not None:
            out["media_s"] = round(b["playback-time"] - a["playback-time"], 2)
    elif s.startswith("vlc"):
        for k in ("displayed", "lost", "video_decoded"):
            if num(a.get(k)) is not None and num(b.get(k)) is not None:
                out[k] = b[k] - a[k]
        out["dropped"] = out.get("lost")
        out["shown"] = out.get("displayed")
    elif s == "utsushi":
        for k in ("shown", "dropped", "decoded", "presented", "discarded"):
            if num(a.get(k)) is not None and num(b.get(k)) is not None:
                out[k] = b[k] - a[k]
        if num(a.get("position")) is not None and num(b.get("position")) is not None:
            out["media_s"] = round(b["position"] - a["position"], 2)
        out["decoder"] = b.get("video_decoder")
        out["path"] = b.get("path")
        out["gpu"] = b.get("gpu")
    return out


def load(dirs):
    recs = []
    for d in dirs:
        batch = os.path.basename(os.path.normpath(d))
        p = os.path.join(d, "results.jsonl")
        if not os.path.exists(p):
            continue
        for line in open(p):
            r = json.loads(line)
            r["batch"] = batch
            recs.append(r)
    return recs


def row(r):
    s = r["spec"]
    p = r.get("power") or {}
    dur = p.get("duration_s") or s.get("window_s", 60)
    cpu = r.get("cpu") or {}
    g = r.get("gpu") or {}

    def c(role, k):
        return (cpu.get(role) or {}).get(k)

    pw_cpu = sum((v or {}).get("cpu_s", 0) for k, v in cpu.items() if k.startswith("pw:"))
    pw_sw = sum(((v or {}).get("vol_switches", 0) + (v or {}).get("invol_switches", 0)) for k, v in cpu.items()
                if k.startswith("pw:"))
    session_cpu = sum((v or {}).get("cpu_s", 0) for v in cpu.values())
    pl_eng = (g.get("player") or {}).get("engine_ns") or {}
    co_eng = (g.get("compositor") or {}).get("engine_ns") or {}
    xw_eng = (g.get("xwayland") or {}).get("engine_ns") or {}
    f = frames(r)
    mem = r.get("player_memory") or {}
    cs = r.get("compositor_stats")
    comp_frames = cs[0].get("frames_rendered") if isinstance(cs, list) and cs else None
    return {
        "batch": r["batch"], "round": s.get("round"), "tag": r["tag"], "ok": r.get("ok"),
        "compositor": s.get("compositor", "aizome"), "mode": s.get("mode", "play"), "sound": bool(s.get("sound")),
        "clip": clip_name(s),
        # mpv's dmabuf-wayland put full screen by the compositor (its own
        # --fs didn't take under Aizome) is reported apart.
        "subject": s["subject"] + ("-fs" if s.get("ensure_fullscreen") and s["subject"].startswith("mpv") else ""),
        "window_s": round(dur, 2), "mean_w": p.get("mean_w"), "energy_j": p.get("energy_j"),
        "p50_w": p.get("p50_w"), "p90_w": p.get("p90_w"),
        "system_cpu_pct": round(100 * p.get("system_busy_cpu_s", 0) / dur, 2) if p else None,
        "session_cpu_pct": round(100 * session_cpu / dur, 2),
        "player_cpu_pct": c("player", "cpu_pct"), "player_switches_s": c("player", "switches_per_s"),
        "player_wakeups_s": c("player", "wakeups_per_s"),
        "compositor_cpu_pct": c("compositor", "cpu_pct"), "compositor_switches_s": c("compositor", "switches_per_s"),
        "shell_cpu_pct": c("shell", "cpu_pct"), "shell_switches_s": c("shell", "switches_per_s"),
        "xwayland_cpu_pct": c("xwayland", "cpu_pct"), "xwayland_switches_s": c("xwayland", "switches_per_s"),
        "pipewire_cpu_pct": round(100 * pw_cpu / dur, 2) if any(k.startswith("pw:") for k in cpu) else None,
        "pipewire_switches_s": round(pw_sw / dur, 1) if any(k.startswith("pw:") for k in cpu) else None,
        "igpu_busy_pct": (r.get("igpu") or {}).get("busy_pct_mean"),
        "igpu_sclk_mhz": (r.get("igpu") or {}).get("sclk_mhz_mean"),
        "tctl_c": (r.get("igpu") or {}).get("tctl_c_mean"),
        "player_gpu_dec_pct": round(100 * pl_eng.get("dec", 0) / 1e9 / dur, 2) if pl_eng else None,
        "player_gpu_gfx_pct": round(100 * pl_eng.get("gfx", 0) / 1e9 / dur, 2) if pl_eng else None,
        "compositor_gpu_gfx_pct": round(100 * co_eng.get("gfx", 0) / 1e9 / dur, 2) if co_eng else None,
        "xwayland_gpu_gfx_pct": round(100 * xw_eng.get("gfx", 0) / 1e9 / dur, 2) if xw_eng else None,
        "total_gpu_gfx_pct": round(100 * (pl_eng.get("gfx", 0) + co_eng.get("gfx", 0) + xw_eng.get("gfx", 0))
                                   / 1e9 / dur, 2),
        "player_devices": " ".join((g.get("player") or {}).get("devices") or []),
        "player_pdevs": " ".join((g.get("player") or {}).get("pdevs") or []),
        "player_nvidia_fds": (g.get("player") or {}).get("nvidia_fds"),
        "player_pss_mib": round(mem["pss_kib"] / 1024, 1) if "pss_kib" in mem else None,
        "player_rss_mib": round(mem["rss_kib"] / 1024, 1) if "rss_kib" in mem else None,
        "compositor_frames": comp_frames,
        "frames_shown": f.get("shown"), "frames_dropped": f.get("dropped"), "frames_late": f.get("late"),
        "media_s": f.get("media_s"),
        "decoder": f.get("decoder"), "vo_or_path": f.get("vo") or f.get("path"), "format": f.get("format"),
        "loadavg_before": " ".join((r.get("quiet") or {}).get("loadavg") or []),
        "busy_cores_before": (r.get("quiet") or {}).get("busy_cores_before"),
        "error": r.get("error"),
    }


def flag_interference(rows, limit=25.0):
    """Mark trials where CPU outside the session (system busy minus the
    session's processes) exceeds the batch's idle baselines' by more than
    `limit` percent of a core: something else ran on the machine. The
    harness itself, the kernel and the user's idle desktop are in the
    baselines' share."""
    ref = {}
    for r in rows:
        if r["ok"] and r["system_cpu_pct"] is not None:
            r["other_cpu_pct"] = round(r["system_cpu_pct"] - r["session_cpu_pct"], 2)
        else:
            r["other_cpu_pct"] = None
        if r["subject"] == "baseline" and r["other_cpu_pct"] is not None:
            ref.setdefault(r["batch"], []).append(r["other_cpu_pct"])
    for r in rows:
        b = ref.get(r["batch"])
        r["interfered"] = bool(b and r["other_cpu_pct"] is not None
                               and r["other_cpu_pct"] > statistics.median(b) + limit)


def baseline_for(rows):
    """Baseline mean power per (batch, compositor, kind, round); kind is
    'sound' when the baseline had the null sink's daemons measured."""
    b = {}
    for r in rows:
        if r["subject"] == "baseline" and r["ok"] and not r["interfered"] and r["mean_w"] is not None:
            b.setdefault((r["batch"], r["compositor"], r["mode"]), {})[r["round"]] = r["mean_w"]
    return b


def seq_of(r):
    return int(r["tag"].split("-", 1)[0])


FLOOR_STEP_W = 0.5


def interpolated_baseline(r, rows):
    """The idle floor at the time of trial r, and whether it held still:
    interpolated by trial number between the nearest good baselines before
    and after it in the same batch and mode (under either compositor: the
    floor is the machine's), the nearer one alone at the ends. If those two
    differ by more than FLOOR_STEP_W the floor moved under the trial (the
    rest of the machine changed) and its power isn't used."""
    bs = sorted((seq_of(b), b["mean_w"]) for b in rows
                if b["batch"] == r["batch"] and b["mode"] == r["mode"]
                and b["subject"] == "baseline" and b["ok"] and not b["interfered"] and b["mean_w"] is not None)
    if not bs:
        return None, False
    n = seq_of(r)
    before = [b for b in bs if b[0] <= n]
    after = [b for b in bs if b[0] >= n]
    if before and after:
        (n0, w0), (n1, w1) = before[-1], after[0]
        w = w0 if n1 == n0 else w0 + (w1 - w0) * (n - n0) / (n1 - n0)
        return w, abs(w1 - w0) > FLOOR_STEP_W
    return (before or after)[-1 if before else 0][1], False


def med(xs):
    xs = [x for x in xs if isinstance(x, (int, float))]
    return round(statistics.median(xs), 3) if xs else None


def rng(xs):
    xs = [x for x in xs if isinstance(x, (int, float))]
    return (round(min(xs), 3), round(max(xs), 3)) if xs else (None, None)


METRICS = ["other_cpu_pct", "mean_w", "baseline_w", "delta_w", "delta_median_w", "delta_j_per_min", "system_cpu_pct", "session_cpu_pct", "player_cpu_pct",
           "player_switches_s", "player_wakeups_s", "compositor_cpu_pct", "compositor_switches_s", "shell_cpu_pct",
           "xwayland_cpu_pct", "pipewire_cpu_pct", "pipewire_switches_s", "igpu_busy_pct", "igpu_sclk_mhz", "tctl_c",
           "player_gpu_dec_pct", "player_gpu_gfx_pct", "compositor_gpu_gfx_pct", "xwayland_gpu_gfx_pct", "total_gpu_gfx_pct",
           "player_pss_mib", "player_rss_mib", "compositor_frames", "frames_shown", "frames_dropped", "frames_late",
           "media_s"]


def main():
    out = sys.argv[1]
    rows = [row(r) for r in load(sys.argv[2:])]
    flag_interference(rows)
    base = baseline_for(rows)
    for r in rows:
        # The reference is the idle floor at the trial's time: interpolated
        # between the baselines interleaved before and after it in the same
        # batch (same compositor and mode), which follows a floor that
        # drifts with temperature or with the rest of the machine. The
        # delta against the batch's median baseline is kept as a check.
        bk = (r["batch"], r["compositor"], r["mode"])
        rb = base.get(bk, {})
        bm = statistics.median(rb.values()) if rb else None
        b, moved = interpolated_baseline(r, rows)
        r["floor_moved"] = moved
        r["baseline_w"] = round(b, 3) if b is not None else None
        r["delta_w"] = (round(r["mean_w"] - b, 3) if (b is not None and r["mean_w"] is not None and not moved)
                        else None)
        r["delta_median_w"] = round(r["mean_w"] - bm, 3) if (bm is not None and r["mean_w"] is not None) else None
        r["delta_j_per_min"] = round(r["delta_w"] * 60, 1) if r["delta_w"] is not None else None
    os.makedirs(out, exist_ok=True)
    keys = list(rows[0].keys()) if rows else []
    with open(f"{out}/trials.csv", "w", newline="") as f:
        w = csv.DictWriter(f, fieldnames=keys)
        w.writeheader()
        for r in sorted(rows, key=lambda r: (r["batch"], r["tag"])):
            w.writerow(r)
    groups = {}
    for r in rows:
        if not r["ok"] or r["interfered"]:
            continue
        groups.setdefault((r["compositor"], r["mode"], r["clip"], r["subject"]), []).append(r)

    def gkey(k):
        comp, mode, clip, subj = k
        return (comp != "aizome", ["play", "pause", "audio"].index(mode) if mode in ("play", "pause", "audio") else 9,
                CLIP_ORDER.index(clip) if clip in CLIP_ORDER else (-1 if clip == "-" else 99),
                ORDER.index(subj) if subj in ORDER else 99)

    summary = []
    for k in sorted(groups, key=gkey):
        rs = groups[k]
        s = {"compositor": k[0], "mode": k[1], "clip": k[2], "subject": k[3], "n": len(rs),
             "failed": sum(1 for r in rows if (r["compositor"], r["mode"], r["clip"], r["subject"]) == k and not r["ok"]),
             "excluded_interference": sum(1 for r in rows if (r["compositor"], r["mode"], r["clip"], r["subject"]) == k
                                          and r["ok"] and r["interfered"]),
             "power_excluded_floor_moved": sum(1 for r in rs if r.get("floor_moved"))}
        for m in METRICS:
            vals = [r.get(m) for r in rs]
            s[m] = med(vals)
            lo, hi = rng(vals)
            s[m + "_min"], s[m + "_max"] = lo, hi
        for m in ("decoder", "vo_or_path", "format", "player_pdevs", "player_devices"):
            s[m] = ";".join(sorted({str(r.get(m)) for r in rs if r.get(m) not in (None, "")}))
        s["nvidia_fds_max"] = max((r.get("player_nvidia_fds") or 0) for r in rs)
        summary.append(s)
    if summary:
        with open(f"{out}/summary.csv", "w", newline="") as f:
            w = csv.DictWriter(f, fieldnames=list(summary[0].keys()))
            w.writeheader()
            w.writerows(summary)
    with open(f"{out}/summary.md", "w") as f:
        f.write(markdown(summary))
    with open(f"{out}/summary-compact.md", "w") as f:
        f.write(compact(summary))
    print(markdown(summary))


def compact(summary):
    """The write-up's tables: one per compositor, mode and clip."""
    lines = []
    cur = None
    for s in summary:
        if s["subject"] == "baseline":
            continue
        head = (s["compositor"], s["mode"], s["clip"])
        if head != cur:
            cur = head
            lines += ["", f"#### {s['compositor']} · {s['mode']} · {s['clip']}", "",
                      "| Player | Δ power W (range) | Δ J/min | Player CPU % | Player wakeups/s | Compositor CPU % | "
                      "Compositor switches/s | Xwayland + PipeWire CPU % | PipeWire switches/s | GPU dec / gfx % | PSS MiB | Dropped | n |",
                      "|---|---|---|---|---|---|---|---|---|---|---|---|---|"]
        others = [x for x in (s.get(m) for m in ("xwayland_cpu_pct", "pipewire_cpu_pct")) if isinstance(x, (int, float))]
        other = sum(others) if others else None
        lines.append(
            f"| {s['subject']} | {rngfmt(s, 'delta_w', 2)} | {fmt(s['delta_j_per_min'], 0)} | "
            f"{fmt(s['player_cpu_pct'], 2)} | {fmt(s['player_wakeups_s'], 0)} | {fmt(s['compositor_cpu_pct'], 2)} | "
            f"{fmt(s['compositor_switches_s'], 0)} | {fmt(other, 2)} | {fmt(s.get('pipewire_switches_s'), 0)} | "
            f"{fmt(s['player_gpu_dec_pct'], 1)} / {fmt(s['total_gpu_gfx_pct'], 1)} | {fmt(s['player_pss_mib'], 0)} | "
            f"{fmt(s['frames_dropped'], 0)} | {s['n']}"
            f"{' (power ' + str(s['n'] - s['power_excluded_floor_moved']) + ')' if s['power_excluded_floor_moved'] else ''} |")
    return "\n".join(lines) + "\n"


def fmt(x, nd=1):
    if x is None:
        return "–"
    if isinstance(x, float):
        return f"{x:.{nd}f}"
    return str(x)


def rngfmt(s, m, nd=1):
    lo, hi = s.get(m + "_min"), s.get(m + "_max")
    if lo is None:
        return fmt(s.get(m), nd)
    return f"{fmt(s.get(m), nd)} ({fmt(lo, nd)}–{fmt(hi, nd)})"


def markdown(summary):
    lines = []
    cur = None
    for s in summary:
        head = (s["compositor"], s["mode"], s["clip"])
        if head != cur:
            cur = head
            lines += ["", f"### {s['compositor']} · {s['mode']} · {s['clip']}", "",
                      "| Subject | n | Power W | Δ W (range) | Δ J/min | Player CPU % | Player sw/s | Comp. CPU % | "
                      "Comp. sw/s | Shell/X/PW CPU % | GPU dec % | GPU gfx % | sclk MHz | PSS MiB | Shown | Dropped | Path |",
                      "|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|"]
        other = "/".join(fmt(s.get(m), 2) for m in ("shell_cpu_pct", "xwayland_cpu_pct", "pipewire_cpu_pct"))
        path = " ".join(x for x in (s.get("decoder"), s.get("vo_or_path"), s.get("format")) if x)
        lines.append(
            f"| {s['subject']} | {s['n']} | {fmt(s['mean_w'], 2)} | {rngfmt(s, 'delta_w', 2)} | "
            f"{fmt(s['delta_j_per_min'], 0)} | {fmt(s['player_cpu_pct'], 2)} | {fmt(s['player_switches_s'], 0)} | "
            f"{fmt(s['compositor_cpu_pct'], 2)} | {fmt(s['compositor_switches_s'], 0)} | {other} | "
            f"{fmt(s['player_gpu_dec_pct'], 1)} | {fmt(s['total_gpu_gfx_pct'], 1)} | {fmt(s['igpu_sclk_mhz'], 0)} | "
            f"{fmt(s['player_pss_mib'], 0)} | {fmt(s['frames_shown'], 0)} | {fmt(s['frames_dropped'], 0)} | {path} |")
    return "\n".join(lines) + "\n"


if __name__ == "__main__":
    main()
