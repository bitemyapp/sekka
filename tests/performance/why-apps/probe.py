#!/usr/bin/env python3
"""Launch programs in a private headless sway, as design/window-cost.py
does, and keep what explains their start: per-thread CPU at the first
frame, their stderr (WAYLAND_DEBUG, LD_DEBUG, stamps), and optionally a
full memory map after they settle.

    probe.py OUTDIR VARIANTS.json [--reps N] [--settle S] [--smaps] [--linger S]

VARIANTS.json is a list of {"name", "cmd": [...], "env": {...}} (an env value
of null unsets a variable the session would pass on); "cmd" may
start with a profiler (perf record, strace, heaptrack), which "kill":
"window" leaves to finish: only the process that mapped the window is sent
SIGTERM. Variants run in turn, A B C ... then C B A ..., after one
unmeasured warm-up each.

For each run, in OUTDIR/runs.jsonl (one JSON object a line):
- t0: CLOCK_MONOTONIC and CLOCK_REALTIME (ns) just before the fork, so
  traces stamped with either clock line up with the run;
- first_frame_ms: t0 to sway's IPC window `new` event for a process of the
  run (sway maps an xdg toplevel on its first commit with a buffer);
- threads_at_map: every thread of every process of the run at that event,
  with its name and schedstat (run ns, run-queue wait ns, timeslices);
- with --settle S: after S seconds, smaps_rollup summed, and with --smaps
  (first measured rep of each variant) each process's /proc smaps, status
  and thread stack pointers copied to OUTDIR/NAME.PID.{smaps,status,sp};
- the run's stderr in OUTDIR/NAME-REP.stderr.

A program that exits by itself (stamps.so with STAMPS_EXIT=1) ends its
run when it exits. Sway, the D-Bus daemon and everything they start run
in process groups of their own, all killed at the end, with anything else
left holding the private runtime directory.
"""
import json
import os
import shutil
import signal
import subprocess
import sys
import tempfile
import threading
import time


def mono_ns():
    return time.clock_gettime_ns(time.CLOCK_MONOTONIC)


def descendants(pid):
    out, todo = [pid], [pid]
    while todo:
        p = todo.pop()
        kids = []
        try:
            tasks = os.listdir(f"/proc/{p}/task")
        except OSError:
            tasks = []
        for t in tasks:
            try:
                with open(f"/proc/{p}/task/{t}/children") as f:
                    kids += [int(x) for x in f.read().split()]
            except OSError:
                pass
        out += kids
        todo += kids
    return out


def read(path):
    try:
        with open(path) as f:
            return f.read()
    except OSError:
        return None


def threads_of(pids):
    out = []
    for p in pids:
        exe = None
        try:
            exe = os.readlink(f"/proc/{p}/exe")
        except OSError:
            pass
        try:
            tasks = os.listdir(f"/proc/{p}/task")
        except OSError:
            continue
        for t in tasks:
            s = read(f"/proc/{p}/task/{t}/schedstat")
            c = read(f"/proc/{p}/task/{t}/comm")
            if s is None:
                continue
            run, wait, slices = (int(x) for x in s.split()[:3])
            out.append({"pid": p, "tid": int(t), "comm": (c or "").strip(), "exe": exe,
                        "run_ns": run, "wait_ns": wait, "slices": slices})
    return out


def mapped_objects(pids):
    libs = set()
    for p in pids:
        for line in (read(f"/proc/{p}/maps") or "").splitlines():
            parts = line.split()
            if len(parts) >= 6 and ".so" in parts[5]:
                libs.add(parts[5])
    return libs


def rollup(pid):
    vals = {}
    for line in (read(f"/proc/{pid}/smaps_rollup") or "").splitlines():
        parts = line.split()
        if len(parts) >= 3 and parts[2] == "kB":
            vals[parts[0].rstrip(":")] = int(parts[1])
    return vals


class Events:
    def __init__(self, env):
        self.proc = subprocess.Popen(["swaymsg", "-t", "subscribe", "-m", '["window"]'], env=env,
                                     stdout=subprocess.PIPE, text=True, start_new_session=True)
        self.cv = threading.Condition()
        self.events = []
        threading.Thread(target=self.read, daemon=True).start()

    def read(self):
        for line in self.proc.stdout:
            t = mono_ns()
            try:
                ev = json.loads(line)
            except ValueError:
                continue
            with self.cv:
                self.events.append((t, ev))
                self.cv.notify_all()

    def wait_new(self, pids, since, alive, timeout=30.0):
        deadline = time.monotonic() + timeout
        dead_since = None
        with self.cv:
            while True:
                for t, ev in self.events:
                    c = ev.get("container", {})
                    if ev.get("change") == "new" and t >= since and c.get("pid") in pids():
                        return t, c
                if not alive():
                    dead_since = dead_since or time.monotonic()
                    if time.monotonic() - dead_since > 1.0:
                        return None, None
                left = deadline - time.monotonic()
                if left <= 0:
                    return None, None
                self.cv.wait(min(left, 0.05))


def killgroup(p, sig=signal.SIGTERM):
    try:
        os.killpg(p.pid, sig)
    except (ProcessLookupError, PermissionError):
        pass


def run_one(out, name, rep, v, env, events, settle, smaps, linger):
    renv = dict(env)
    for k, val in v.get("env", {}).items():
        # null unsets a variable the session environment would pass on.
        if val is None:
            renv.pop(k, None)
        else:
            renv[k] = val
    tag = f"{name}-{rep}"
    for k, val in list(renv.items()):
        if isinstance(val, str) and "{tag}" in val:
            renv[k] = val.replace("{tag}", tag).replace("{out}", out)
    cmd = [c.replace("{tag}", tag).replace("{out}", out) for c in v["cmd"]]
    err = open(f"{out}/{tag}.stderr", "w")
    t0_real = time.clock_gettime_ns(time.CLOCK_REALTIME)
    t0 = mono_ns()
    p = subprocess.Popen(cmd, env=renv, stdout=subprocess.DEVNULL, stderr=err, start_new_session=True)
    t_map, c = events.wait_new(lambda: set(descendants(p.pid)), t0, lambda: p.poll() is None)
    rec = {"name": name, "rep": rep, "t0_mono_ns": t0, "t0_real_ns": t0_real, "cmd": cmd,
           "env": v.get("env", {})}
    if t_map is None:
        rec["first_frame_ms"] = None
        rec["exited"] = p.poll()
    else:
        procs = descendants(p.pid)
        rec["first_frame_ms"] = round((t_map - t0) / 1e6, 3)
        rec["t_map_mono_ns"] = t_map
        rec["window_pid"] = c.get("pid")
        rec["threads_at_map"] = threads_of(procs)
        rec["shared_objects_at_map"] = sorted(mapped_objects(procs))
        if settle > 0:
            time.sleep(settle)
            procs = descendants(p.pid)
            mem = {}
            for q in procs:
                for k, val in rollup(q).items():
                    mem[k] = mem.get(k, 0) + val
            rec["rollup_kib"] = mem
            rec["threads_after_settle"] = threads_of(procs)
            if smaps:
                rec["smaps_files"] = []
                for q in procs:
                    base = f"{out}/{name}.{q}"
                    for src, ext in (("smaps", "smaps"), ("status", "status"), ("maps", "maps")):
                        s = read(f"/proc/{q}/{src}")
                        if s is not None:
                            with open(f"{base}.{ext}", "w") as f:
                                f.write(s)
                    # Each thread's stack pointer (from /proc/.../syscall)
                    # names the mapping that is its stack.
                    with open(f"{base}.sp", "w") as f:
                        for t in os.listdir(f"/proc/{q}/task") if os.path.isdir(f"/proc/{q}/task") else []:
                            s = read(f"/proc/{q}/task/{t}/syscall") or ""
                            cm = (read(f"/proc/{q}/task/{t}/comm") or "").strip()
                            parts = s.split()
                            sp = parts[-2] if len(parts) >= 3 else "-"
                            f.write(f"{t} {sp} {cm}\n")
                    rec["smaps_files"].append(base)
        if linger > 0:
            time.sleep(linger)
    # A profiler wrapping the program ("kill": "window") is left to write
    # its data: only the process that mapped the window is signalled, and
    # the wrapper is waited for.
    grace = 5
    if v.get("kill") == "window" and rec.get("window_pid"):
        try:
            os.kill(rec["window_pid"], signal.SIGTERM)
        except ProcessLookupError:
            pass
        grace = 60
    else:
        killgroup(p)
    try:
        p.wait(grace)
    except subprocess.TimeoutExpired:
        killgroup(p)
        try:
            p.wait(5)
        except subprocess.TimeoutExpired:
            killgroup(p, signal.SIGKILL)
            p.wait()
    killgroup(p, signal.SIGKILL)
    err.close()
    time.sleep(0.3)
    return rec


def main():
    args = sys.argv[1:]
    out = os.path.abspath(args.pop(0))
    variants = json.load(open(args.pop(0)))
    reps, settle, smaps, linger, warm = 5, 0.0, False, 0.0, True
    while args:
        a = args.pop(0)
        if a == "--reps":
            reps = int(args.pop(0))
        elif a == "--settle":
            settle = float(args.pop(0))
        elif a == "--smaps":
            smaps = True
        elif a == "--linger":
            linger = float(args.pop(0))
        elif a == "--no-warmup":
            warm = False
        else:
            sys.exit(f"unknown argument {a}")
    os.makedirs(out, exist_ok=True)
    tmp = tempfile.mkdtemp(prefix="wa.", dir="/tmp/claude-1000")
    env = {k: v for k, v in os.environ.items() if k not in ("WAYLAND_DISPLAY", "DISPLAY", "WAYLAND_SOCKET")}
    env.update({
        "XDG_RUNTIME_DIR": tmp, "HOME": f"{tmp}/home", "XDG_CONFIG_HOME": f"{tmp}/home/.config",
        "XDG_CACHE_HOME": f"{tmp}/home/.cache", "XDG_DATA_HOME": f"{tmp}/home/.local/share",
        "XDG_STATE_HOME": f"{tmp}/home/.local/state",
        "WLR_BACKENDS": "headless", "WLR_RENDERER": "gles2", "WLR_LIBINPUT_NO_DEVICES": "1",
        "WLR_RENDER_DRM_DEVICE": os.environ.get("WLR_RENDER_DRM_DEVICE", "/dev/dri/renderD128"),
        "__EGL_VENDOR_LIBRARY_FILENAMES": os.environ.get(
            "__EGL_VENDOR_LIBRARY_FILENAMES", "/run/opengl-driver/share/glvnd/egl_vendor.d/50_mesa.json"),
        "GDK_BACKEND": "wayland", "QT_QPA_PLATFORM": "wayland", "XDG_SESSION_TYPE": "wayland",
    })
    os.makedirs(env["XDG_CONFIG_HOME"], exist_ok=True)
    conf = f"{tmp}/sway.conf"
    with open(conf, "w") as f:
        f.write("output HEADLESS-1 resolution 1920x1080 scale 1\nxwayland disable\n"
                "default_border none\nfocus_on_window_activation focus\n"
                "for_window [title=\".*\"] floating enable\n")
    sway = subprocess.Popen(["sway", "-c", conf], env=env, stdout=subprocess.DEVNULL,
                            stderr=open(f"{out}/sway.log", "w"), start_new_session=True)
    bus = None
    events = None
    jl = open(f"{out}/runs.jsonl", "a")
    try:
        for _ in range(100):
            socks = [s for s in os.listdir(tmp) if s.startswith("sway-ipc")]
            ways = [s for s in os.listdir(tmp) if s.startswith("wayland-") and not s.endswith(".lock")]
            if socks and ways:
                break
            time.sleep(0.1)
        env["SWAYSOCK"] = f"{tmp}/{socks[0]}"
        env["WAYLAND_DISPLAY"] = ways[0]
        bus = subprocess.Popen(["dbus-daemon", "--session", "--nofork", "--nopidfile",
                                f"--address=unix:path={tmp}/bus", "--print-address"],
                               env=env, stdout=subprocess.PIPE, text=True, start_new_session=True)
        env["DBUS_SESSION_BUS_ADDRESS"] = bus.stdout.readline().strip()
        time.sleep(0.5)
        events = Events(env)
        time.sleep(0.3)
        meta = {"meta": True, "date": time.strftime("%Y-%m-%dT%H:%M:%S%z"), "host": os.uname().nodename,
                "kernel": os.uname().release, "load_before": open("/proc/loadavg").read().split()[:3],
                "reps": reps, "settle_s": settle, "variants": variants,
                "sway": subprocess.run(["sway", "--version"], capture_output=True, text=True).stdout.strip()}
        jl.write(json.dumps(meta) + "\n")
        if warm:
            for v in variants:
                r = run_one(out, v["name"], "warm", v, env, events, 0.0, False, 0.0)
                r["warmup"] = True
                jl.write(json.dumps(r) + "\n")
        for i in range(reps):
            order = variants if i % 2 == 0 else list(reversed(variants))
            for v in order:
                r = run_one(out, v["name"], i, v, env, events, settle, smaps and i == 0, linger)
                jl.write(json.dumps(r) + "\n")
                jl.flush()
                print(v["name"], i, r.get("first_frame_ms"), flush=True)
        jl.write(json.dumps({"meta": True, "load_after": open("/proc/loadavg").read().split()[:3]}) + "\n")
    finally:
        jl.close()
        for p in (events.proc if events else None, bus, sway):
            if p is not None:
                killgroup(p)
        time.sleep(0.5)
        for p in (events.proc if events else None, bus, sway):
            if p is not None:
                killgroup(p, signal.SIGKILL)
        # Anything else still holding the private runtime directory (an
        # activated service that left its group): find it by environment.
        mark = f"XDG_RUNTIME_DIR={tmp}".encode()
        for d in os.listdir("/proc"):
            if not d.isdigit() or int(d) == os.getpid():
                continue
            try:
                with open(f"/proc/{d}/environ", "rb") as f:
                    if mark in f.read().split(b"\0"):
                        os.kill(int(d), signal.SIGKILL)
            except OSError:
                pass
        shutil.rmtree(tmp, ignore_errors=True)


if __name__ == "__main__":
    main()
