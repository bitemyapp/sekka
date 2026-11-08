#!/usr/bin/env python3
"""What an idle Aizome session costs: a headless compositor with its shell,
left alone, read from /proc and the compositor's own frame statistics.

    session-idle.py OUT.json AIZOME_BIN WxH@S [WxH@S ...] [--settle S] [--idle S] [--reps N]

For each output size, REPS times: start `aizome --backend headless` in a
private environment (XDG_RUNTIME_DIR, HOME, config: a fresh user, Aizome's
defaults), which starts the shell and xwayland-satellite as a login session
does. After SETTLE seconds, reset the compositor's frame statistics, leave
the session alone for IDLE seconds, then record for every process of the
session: CPU time (utime+stime, and schedstat over live threads),
context switches (every thread, voluntary and not) and PSS with its
anon/file/shmem parts; and the frames the compositor rendered meanwhile.

The compositor renders with GLES on AIZOME_RENDER_NODE (default the AMD
iGPU, /dev/dri/renderD128) with Mesa's EGL.
"""
import json
import os
import signal
import subprocess
import sys
import tempfile
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from aizome_session import private_bus_shell  # noqa: E402

def descendants(pid):
    out, todo = [pid], [pid]
    while todo:
        p = todo.pop()
        # Every thread's children: a child started by a thread other than
        # the main one (a worker, a loader) is listed under that thread.
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


def comm(pid):
    try:
        with open(f"/proc/{pid}/comm") as f:
            return f.read().strip()
    except OSError:
        return "?"


def ticks(pid):
    try:
        with open(f"/proc/{pid}/stat") as f:
            fields = f.read().rsplit(")", 1)[1].split()
        return (int(fields[11]) + int(fields[12])) / os.sysconf("SC_CLK_TCK")
    except OSError:
        return 0.0


def per_thread(pid, path, parse):
    total = 0
    try:
        tasks = os.listdir(f"/proc/{pid}/task")
    except OSError:
        return 0
    for t in tasks:
        try:
            with open(f"/proc/{pid}/task/{t}/{path}") as f:
                total += parse(f.read())
        except (OSError, ValueError, IndexError):
            pass
    return total


def sched(pid):
    return per_thread(pid, "schedstat", lambda s: int(s.split()[0])) / 1e9


def switches(pid):
    def parse(s):
        n = 0
        for line in s.splitlines():
            if line.startswith(("voluntary_ctxt_switches", "nonvoluntary_ctxt_switches")):
                n += int(line.split()[1])
        return n
    return per_thread(pid, "status", parse)


def rollup(pid):
    vals = {}
    try:
        with open(f"/proc/{pid}/smaps_rollup") as f:
            for line in f:
                parts = line.split()
                if len(parts) >= 3 and parts[2] == "kB":
                    vals[parts[0].rstrip(":")] = int(parts[1])
    except OSError:
        pass
    return vals


def threads(pid):
    try:
        return len(os.listdir(f"/proc/{pid}/task"))
    except OSError:
        return 0


def ctl(bindir, env, *args):
    r = subprocess.run([f"{bindir}/aizomectl", "--json", *args], env=env, capture_output=True, text=True, timeout=10)
    return json.loads(r.stdout) if r.stdout.strip() else {"stderr": r.stderr}


def one(bindir, output, settle, idle):
    # The Wayland, IPC and D-Bus socket paths must fit in sockaddr_un.
    tmp = tempfile.mkdtemp(prefix="si.", dir="/tmp")
    env = {k: v for k, v in os.environ.items() if k not in ("WAYLAND_DISPLAY", "DISPLAY", "WAYLAND_SOCKET",
                                                           "AIZOME_SOCKET", "AIZOME_SHELL_SOCKET", "DBUS_SESSION_BUS_ADDRESS")}
    env.update({
        "XDG_RUNTIME_DIR": tmp, "HOME": f"{tmp}/home", "XDG_CONFIG_HOME": f"{tmp}/home/.config",
        "XDG_CACHE_HOME": f"{tmp}/home/.cache", "XDG_DATA_HOME": f"{tmp}/home/.local/share",
        "XDG_STATE_HOME": f"{tmp}/home/.local/state",
        "AIZOME_RENDER_NODE": os.environ.get("AIZOME_RENDER_NODE", "/dev/dri/renderD128"),
        "__EGL_VENDOR_LIBRARY_FILENAMES": os.environ.get(
            "__EGL_VENDOR_LIBRARY_FILENAMES", "/run/opengl-driver/share/glvnd/egl_vendor.d/50_mesa.json"),
        "PATH": f"{bindir}:{os.environ['PATH']}",
    })
    os.makedirs(env["HOME"], exist_ok=True)
    bus = subprocess.Popen(["dbus-daemon", "--session", "--nofork", "--nopidfile", f"--address=unix:path={tmp}/bus",
                            "--print-address"], env=env, stdout=subprocess.PIPE, text=True)
    env["DBUS_SESSION_BUS_ADDRESS"] = bus.stdout.readline().strip()
    env["AIZOME_SHELL_BIN"] = private_bus_shell(tmp, env["DBUS_SESSION_BUS_ADDRESS"], f"{bindir}/aizome-shell")
    comp = subprocess.Popen([f"{bindir}/aizome", "--backend", "headless", "--headless-output", output,
                             "--renderer", "gles", "--socket", "wayland-bench"],
                            env=env, stdout=subprocess.DEVNULL, stderr=open(f"{tmp}/aizome.log", "w"),
                            start_new_session=True)
    env["AIZOME_SOCKET"] = f"{tmp}/aizome.wayland-bench.sock"
    try:
        time.sleep(settle)
        stats0 = ctl(bindir, env, "stats", "--reset")
        procs = descendants(comp.pid)
        before = {p: (ticks(p), sched(p), switches(p)) for p in procs}
        t0 = time.monotonic()
        time.sleep(idle)
        t1 = time.monotonic()
        stats = ctl(bindir, env, "stats")
        dt = t1 - t0
        rows = []
        for p in procs:
            if p not in before:
                continue
            tk, sc, sw = ticks(p), sched(p), switches(p)
            m = rollup(p)
            rows.append({
                "comm": comm(p), "threads": threads(p),
                "cpu_ms_per_s": round((tk - before[p][0]) * 1000 / dt, 3),
                "cpu_sched_ms_per_s": round((sc - before[p][1]) * 1000 / dt, 4),
                "switches_per_s": round((sw - before[p][2]) / dt, 2),
                "pss_kib": m.get("Pss", 0), "rss_kib": m.get("Rss", 0), "pss_anon_kib": m.get("Pss_Anon", 0),
                "pss_file_kib": m.get("Pss_File", 0), "pss_shmem_kib": m.get("Pss_Shmem", 0),
            })
        return {"output": output, "idle_s": round(dt, 3), "processes": rows, "frame_stats": stats,
                "load": open("/proc/loadavg").read().split()[:3]}
    finally:
        os.killpg(comp.pid, signal.SIGTERM)
        try:
            comp.wait(10)
        except subprocess.TimeoutExpired:
            os.killpg(comp.pid, signal.SIGKILL)
            comp.wait()
        bus.terminate()
        time.sleep(1)


def main():
    args = sys.argv[1:]
    out, bindir = args.pop(0), args.pop(0)
    settle, idle, reps, outputs = 30.0, 60.0, 3, []
    while args:
        a = args.pop(0)
        if a == "--settle":
            settle = float(args.pop(0))
        elif a == "--idle":
            idle = float(args.pop(0))
        elif a == "--reps":
            reps = int(args.pop(0))
        else:
            outputs.append(a)
    record = {"date": time.strftime("%Y-%m-%dT%H:%M:%S%z"), "host": os.uname().nodename,
              "kernel": os.uname().release, "aizome": os.path.realpath(bindir), "settle_s": settle,
              "render_node": os.environ.get("AIZOME_RENDER_NODE", "/dev/dri/renderD128"), "runs": []}
    for i in range(reps):
        for o in outputs:
            r = one(bindir, o, settle, idle)
            r["rep"] = i
            record["runs"].append(r)
            print(json.dumps({"output": o, "rep": i, "frames": r["frame_stats"], "procs": [
                (p["comm"], p["cpu_sched_ms_per_s"], p["switches_per_s"], p["pss_kib"]) for p in r["processes"]]}),
                flush=True)
    with open(out, "w") as f:
        json.dump(record, f, indent=1)


if __name__ == "__main__":
    main()
