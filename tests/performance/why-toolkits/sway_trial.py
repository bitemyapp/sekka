#!/usr/bin/env python3
"""One trial of the controlled comparison (every toolkit under the same
private headless sway): docs/performance/why-toolkits.md.

    sway_trial.py SUBJECT SCENARIO OUTDIR [--hz HZ] [--res WxH] [--scale S]
                  [--keys N] [--seconds S] [--size W H] [--file F] [--seed N]

Scenarios:
  latency     keystrokes through sway's virtual keyboard, each timed to the
              first frame sway composes whose copy (wlr-screencopy, the
              window's region) differs: no proxy, nothing in the program
  keys        keystrokes with the tracing proxy between the program and
              sway, the screen not copied: when each key reached the program,
              its commit, damage and buffer, the frame callback after it;
              CPU, GPU and socket power over the typing, and 5 s idle before
  scroll      the program scrolls its list by itself (--scroll ROWS MS, given
              by --args); the proxy counts its frames; CPU, GPU, power
  resize      sway resizes the window 60 times a second; the proxy times
              configure to the commit at the new size
  idle        the program left alone for --seconds (caret blinking where
              --blink): what it and sway do
  wheel       wheel notches through sway's virtual pointer, timed by
              screencopy as for latency (real apps' scrolling)

Writes OUTDIR/result.json plus the logs (sway, program, proxy, observer,
power)."""
import argparse
import json
import os
import random
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import tk  # noqa: E402

KEY_A, KEY_BACKSPACE = 30, 14


PERF = f"{tk.ROOTS}/perf/bin/perf"
STAT_EVENTS = "task-clock,cycles:u,instructions:u,cache-references:u,cache-misses:u,branch-misses:u"


def perf_start(mode, pids, out, seconds):
    """perf on `pids` for `seconds` (user space only: perf_event_paranoid is 2
    here). record: DWARF call chains, about 1 kHz of CPU time; stat: counters."""
    if mode == "record":
        argv = [PERF, "record", "-q", "-e", "cycles:u", "-F", "997", "--call-graph", "dwarf,12288",
                "-o", out + ".data", "-p", ",".join(map(str, pids)), "--", "sleep", str(seconds)]
    else:
        argv = [PERF, "stat", "-x", ",", "-e", STAT_EVENTS, "--per-thread", "-o", out + ".csv",
                "-p", ",".join(map(str, pids)), "--", "sleep", str(seconds)]
    return tk.Group(argv, {"PATH": "/run/current-system/sw/bin"}, log=out + ".log", cpus=tk.CPU_HARNESS)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("subject")
    ap.add_argument("scenario")
    ap.add_argument("out")
    ap.add_argument("--hz", type=int, default=60)
    ap.add_argument("--res", default="1920x1080")
    ap.add_argument("--scale", type=int, default=1)
    ap.add_argument("--keys", type=int, default=150)
    ap.add_argument("--rate", type=float, default=0, help="keys a second (0: paced by responses)")
    ap.add_argument("--seconds", type=float, default=10)
    ap.add_argument("--args", default="", help="the program's arguments")
    ap.add_argument("--blink", action="store_true")
    ap.add_argument("--settle", type=float, default=3.0)
    ap.add_argument("--seed", type=int, default=1)
    ap.add_argument("--notch", type=float, default=15.0)
    ap.add_argument("--rt", default=f"{tk.TK}/rt")
    ap.add_argument("--no-tick", action="store_true", help="leave sway's output idle between commits")
    ap.add_argument("--full-copy", action="store_true", help="copy the whole output every frame, as the terminal "
                    "comparison's observer did, rather than the rows a key changes")
    ap.add_argument("--perf", choices=["record", "stat"], help="profile the program and sway over the measured "
                    "window (attribution only: these runs' timings aren't reported)")
    a = ap.parse_args()
    os.makedirs(a.out, exist_ok=True)
    w, h = (int(x) for x in a.res.split("x"))
    rnd = random.Random(a.seed)
    result = dict(subject=a.subject, scenario=a.scenario, hz=a.hz, res=a.res, scale=a.scale, args=a.args, perf=a.perf,
                  load_before=tk.loadavg(), started=time.time())
    tk.fonts_conf(f"{tk.TK}/fontconfig/fonts.conf")
    sw = bus = px = app = obs = ppt = None
    try:
        sw = tk.Sway(a.rt, w, h, a.scale, a.hz, log=f"{a.out}/sway.log").start()
        bus = tk.Bus(a.rt).start()
        display = sw.display
        if a.scenario in ("keys", "scroll", "resize", "idle"):
            px = tk.Proxy(a.rt, sw.display, "tkp", f"{a.out}/trace.log").start()
            display = "tkp"
        obs = tk.Observer(a.rt, sw.display, f"{a.out}/observer.log")
        if not a.no_tick:
            obs.send("tick on")
        ppt = tk.Group([f"{tk.TOOLS}/ppt-sampler", "10", f"{a.out}/ppt.tsv"], {}, cpus=tk.CPU_HARNESS)
        # sway with the ticker alone: the compositor's baseline.
        time.sleep(1)
        b0 = tk.sample([sw.pid])
        time.sleep(3)
        b1 = tk.sample([sw.pid])
        result["comp_baseline"] = tk.diff(b0, b1)
        result["baseline_window"] = [b0["t"], b1["t"]]
        home = f"{tk.TK}/home/{a.subject}" + ("-blink" if a.blink else "")
        tk.prepare_home(home, blink=a.blink)
        argv, app_id, family = tk.subjects()[a.subject]
        env = tk.client_env(a.rt, display, home, {"TK_BLINK": "1"} if a.blink else None)
        app = tk.Group(argv + a.args.split(), env, log=f"{a.out}/app.log", cpus=tk.CPU_APP)
        win = sw.find(app_id)
        if not win:
            raise RuntimeError(f"no window for {app_id}")
        if a.scenario == "resize":
            sw.command(f'[app_id="{app_id}"] resize set width 1200 px height 800 px')
        time.sleep(a.settle)
        win = sw.find(app_id)
        result["window"] = win["rect"]
        result["app_pid"] = app.pid
        apps = lambda: [p for p in tk.descendants(app.pid)]  # noqa: E731
        comp = [sw.pid]
        result["app_procs"] = {p: tk.comm(p) for p in apps()}
        if a.scenario == "latency" or a.scenario == "wheel":
            r = win["rect"]
            s = a.scale
            # The window's region of the output, in output pixels (even widths).
            rx, ry, rw, rh = r["x"] * s, r["y"] * s, (r["width"] * s) // 2 * 2, r["height"] * s
            if a.scenario == "wheel":
                # The pointer over the file list (four fifths across: Hako's
                # column view has the folder's files in its last column).
                ow, oh = w // a.scale, h // a.scale
                obs.send(f"extent {ow} {oh}")
                obs.send(f"motion {r['x'] + r['width'] * 4 // 5} {r['y'] + r['height'] // 2}")
                time.sleep(0.3)
            obs.send(f"region {rx} {ry} {rw} {rh}")
            obs.send("calib on")
            obs.send("watch")
            frames = [e for e in obs.drain(0.5) if e["event"] == "frame"]
            last_hash = frames[-1]["hash"] if frames else None
            trials = []
            warm = 10
            box = None
            w0 = None
            for i in range(a.keys + warm):
                if i == warm:
                    w0 = (tk.sample(apps()), tk.sample(comp))
                if i == warm:
                    # The rows the warm-up's responses changed: copy only
                    # them (and a margin) from now on, to keep each copy small.
                    obs.send("unwatch")
                    obs.drain(0.1)
                    if a.full_copy:
                        obs.send(f"region 0 0 {w} {h}")
                        result["region"] = [0, 0, w, h]
                    elif box:
                        y0, y1 = max(0, box[1] - 16), min(rh, box[3] + 17)
                        obs.send(f"region {rx} {ry + y0} {rw} {y1 - y0}")
                        result["region"] = [rx, ry + y0, rw, y1 - y0]
                    obs.send("calib off")
                    obs.send("watch")
                    frames = [e for e in obs.drain(0.3) if e["event"] == "frame"]
                    last_hash = frames[-1]["hash"] if frames else None
                if a.scenario == "latency":
                    code = KEY_A if (i // 10) % 2 == 0 else KEY_BACKSPACE
                    obs.send(f"key {code}")
                else:
                    obs.send(f"axis {a.notch}")
                inject = None
                resp = None
                seen = []
                t_end = time.monotonic() + 0.5
                while time.monotonic() < t_end:
                    e = obs.read(max(0.0, t_end - time.monotonic()))
                    if e is None:
                        break
                    if e["event"] in ("key", "axis"):
                        inject = e["inject"]
                    elif e["event"] == "frame":
                        seen.append(e)
                        if i < warm and e.get("box", [-1])[0] >= 0:
                            b = e["box"]
                            box = b if box is None else [min(box[0], b[0]), min(box[1], b[1]), max(box[2], b[2]), max(box[3], b[3])]
                        if not inject or e["t"] <= inject:
                            last_hash = e["hash"]
                        elif e["hash"] != last_hash and resp is None:
                            resp = e
                            # Keep reading a while: frames that follow (a
                            # program that takes more than one frame to
                            # show all of its response).
                            t_end = min(t_end, time.monotonic() + 0.08)
                # The whole response: the first frame after the key that
                # shows what the region settled to.
                full = None
                if resp:
                    last_hash = seen[-1]["hash"]
                    full = next((f for f in seen if f["t"] > inject and f["hash"] == last_hash), None)
                if i >= warm:
                    trials.append(dict(inject=inject, frame=resp["t"] if resp else None, ticks=resp.get("ticks") if resp else None,
                                       full=full["t"] if full else None,
                                       recv=resp["recv"] if resp else None,
                                       frames_after=[(f["t"] - inject) / 1e6 for f in seen if inject and f["t"] > inject],
                                       changed=[f["hash"] != seen[k - 1]["hash"] if k else True for k, f in enumerate(seen)]))
                time.sleep(rnd.uniform(0.04, 0.09))
            obs.send("unwatch")
            w1 = (tk.sample(apps()), tk.sample(comp))
            result["window_cost"] = dict(app=tk.diff(w0[0], w1[0]), comp=tk.diff(w0[1], w1[1]), events=a.keys)
            result["trials"] = trials
            lat = sorted((t["frame"] - t["inject"]) / 1e6 for t in trials if t["frame"])
            result["missed"] = sum(1 for t in trials if not t["frame"])
            result["latency_ms"] = lat
            result["latency_full_ms"] = sorted((t["full"] - t["inject"]) / 1e6 for t in trials if t.get("full"))
        elif a.scenario == "keys":
            # 5 s idle, then typing; the screen isn't copied.
            i0 = tk.sample(apps()), tk.sample(comp)
            time.sleep(5)
            i1 = tk.sample(apps()), tk.sample(comp)
            result["idle"] = dict(app=tk.diff(i0[0], i1[0]), comp=tk.diff(i0[1], i1[1]))
            perfs = []
            if a.perf:
                dur = a.keys / (a.rate or 9.5) + 0.3
                perfs = [perf_start(a.perf, apps(), f"{a.out}/perf-app", dur),
                         perf_start(a.perf, comp, f"{a.out}/perf-comp", dur)]
                time.sleep(0.3)
            t0 = (tk.sample(apps()), tk.sample(comp))
            injects = []
            for i in range(a.keys):
                code = KEY_A if (i // 10) % 2 == 0 else KEY_BACKSPACE
                obs.send(f"key {code}")
                e = obs.read(1)
                while e and e["event"] != "key":
                    e = obs.read(1)
                injects.append(e["inject"] if e else None)
                time.sleep(1 / a.rate if a.rate else rnd.uniform(0.08, 0.13))
            time.sleep(0.3)
            t1 = (tk.sample(apps()), tk.sample(comp))
            result["typing"] = dict(app=tk.diff(t0[0], t1[0]), comp=tk.diff(t0[1], t1[1]), keys=a.keys,
                                    t0=t0[0]["t"], t1=t1[0]["t"])
            result["idle_window"] = [i0[0]["t"], i1[0]["t"]]
            result["injects"] = injects
            for g in perfs:
                g.p.wait()
        elif a.scenario in ("scroll", "resize", "idle"):
            perfs = []
            if a.perf:
                perfs = [perf_start(a.perf, apps(), f"{a.out}/perf-app", a.seconds),
                         perf_start(a.perf, comp, f"{a.out}/perf-comp", a.seconds)]
                time.sleep(0.3)
            t0 = (tk.sample(apps()), tk.sample(comp))
            if a.scenario == "resize":
                period = 1 / 60
                k, t_next = 0, time.monotonic()
                sizes = []
                while time.monotonic() - (t_next - k * period) < a.seconds:
                    width = 900 + 10 * abs((k % 120) - 60)
                    sw.command(f'[app_id="{app_id}"] resize set width {width} px height 800 px')
                    sizes.append((tk.now_ns(), width))
                    k += 1
                    t_next += period
                    time.sleep(max(0, t_next - time.monotonic()))
                result["resizes"] = sizes
            else:
                if a.blink:
                    # A key restarts every toolkit's blinking (GTK's stops
                    # after 10 s without one).
                    obs.send("key 30")
                    time.sleep(0.5)
                    t0 = (tk.sample(apps()), tk.sample(comp))
                time.sleep(a.seconds)
            t1 = (tk.sample(apps()), tk.sample(comp))
            result["window_stats"] = dict(app=tk.diff(t0[0], t1[0]), comp=tk.diff(t0[1], t1[1]),
                                          t0=t0[0]["t"], t1=t1[0]["t"])
            for g in perfs:
                g.p.wait()
        result["load_after"] = tk.loadavg()
    finally:
        for g in (obs, app, px, bus):
            if g:
                g.stop()
        if ppt:
            ppt.stop()
        if sw:
            sw.stop()
    with open(f"{a.out}/result.json", "w") as f:
        json.dump(result, f)


if __name__ == "__main__":
    main()
