#!/usr/bin/env python3
"""The cost of a window: start a program in a private headless sway, time it
to its first frame, then read what it holds and how often it wakes.

    window-cost.py OUT.json NAME=COMMAND [NAME=COMMAND ...] [--reps N] [--idle S]

Each NAME=COMMAND is a shell-split command line. Programs run in turn
(A B C ... A B C ...), after one unmeasured warm-up each. For each run:

- first frame: from just before exec to sway's IPC `window` event `new`
  for the program's pid (sway maps an xdg toplevel on its first commit with
  a buffer), read by a `swaymsg -t subscribe -m` reader thread that stamps
  each line with CLOCK_MONOTONIC on arrival;
- CPU to first frame: the program's utime+stime read at that event (10 ms
  ticks, every thread), and the sum of its live threads' schedstat run time
  (nanoseconds; threads already gone are missed);
- after SETTLE seconds: PSS/RSS and their anon/file/shmem parts
  (smaps_rollup, summed over the program and its descendants), threads,
  shared objects mapped, and whether a GPU driver (Mesa DRI/Gallium, Vulkan
  ICD, NVIDIA GL) is mapped;
- then IDLE seconds with nothing happening: CPU time and context switches
  (voluntary + involuntary, over every thread) per second.

The environment is private: XDG_RUNTIME_DIR, HOME and config/cache/data
homes in a temporary directory; a D-Bus session daemon of its own, started
in that environment after sway (so services it activates, such as the
portal, see only the private sway); no Xwayland. Sway renders with GLES on
WLR_RENDER_DRM_DEVICE (default /dev/dri/renderD128) and the programs' EGL
picks the same vendor (__EGL_VENDOR_LIBRARY_FILENAMES).
"""
import json
import os
import shlex
import signal
import subprocess
import sys
import tempfile
import threading
import time

SETTLE = 3.0


def now():
    return time.clock_gettime(time.CLOCK_MONOTONIC)


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


def cpu_seconds(pid):
    try:
        with open(f"/proc/{pid}/stat") as f:
            fields = f.read().rsplit(")", 1)[1].split()
        return (int(fields[11]) + int(fields[12])) / os.sysconf("SC_CLK_TCK")
    except OSError:
        return 0.0


def sched_seconds(pid):
    """CPU time of the process's live threads, in ns from schedstat (exact,
    but threads that have already exited are not counted)."""
    total = 0
    try:
        tasks = os.listdir(f"/proc/{pid}/task")
    except OSError:
        return 0.0
    for t in tasks:
        try:
            with open(f"/proc/{pid}/task/{t}/schedstat") as f:
                total += int(f.read().split()[0])
        except (OSError, ValueError, IndexError):
            pass
    return total / 1e9


def switches(pid):
    total = 0
    try:
        tasks = os.listdir(f"/proc/{pid}/task")
    except OSError:
        return 0
    for t in tasks:
        try:
            with open(f"/proc/{pid}/task/{t}/status") as f:
                for line in f:
                    if line.startswith(("voluntary_ctxt_switches", "nonvoluntary_ctxt_switches")):
                        total += int(line.split()[1])
        except OSError:
            pass
    return total


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


GPU_MARKERS = ("_dri.so", "libgallium", "libvulkan_", "libGLX_nvidia", "libEGL_nvidia", "libnvidia-glcore", "libnvidia-eglcore")


def mapped(pid):
    libs, gpu = set(), set()
    try:
        with open(f"/proc/{pid}/maps") as f:
            for line in f:
                parts = line.split()
                if len(parts) >= 6 and ".so" in parts[5]:
                    libs.add(parts[5])
                    base = os.path.basename(parts[5])
                    if any(m in base for m in GPU_MARKERS):
                        gpu.add(base)
    except OSError:
        pass
    return libs, gpu


def threads(pid):
    try:
        return len(os.listdir(f"/proc/{pid}/task"))
    except OSError:
        return 0


class Events:
    def __init__(self, env):
        self.proc = subprocess.Popen(["swaymsg", "-t", "subscribe", "-m", '["window"]'], env=env,
                                     stdout=subprocess.PIPE, text=True)
        self.lock = threading.Condition()
        self.events = []
        threading.Thread(target=self.read, daemon=True).start()

    def read(self):
        for line in self.proc.stdout:
            t = now()
            try:
                ev = json.loads(line)
            except ValueError:
                continue
            with self.lock:
                self.events.append((t, ev))
                self.lock.notify_all()

    def wait_new(self, pids, since, timeout=30.0):
        deadline = time.monotonic() + timeout
        with self.lock:
            while True:
                for t, ev in self.events:
                    c = ev.get("container", {})
                    if ev.get("change") == "new" and t >= since and c.get("pid") in pids():
                        return t, c
                left = deadline - time.monotonic()
                if left <= 0:
                    return None, None
                self.lock.wait(left)


def run_one(name, cmd, env, events, idle):
    t0 = now()
    p = subprocess.Popen(cmd, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, start_new_session=True)
    t_map, c = events.wait_new(lambda: set(descendants(p.pid)), t0)
    if t_map is None:
        os.killpg(p.pid, signal.SIGKILL)
        err = p.stderr.read().decode(errors="replace")[-2000:]
        return {"name": name, "error": "no window within 30 s", "stderr": err}
    win_pid = c.get("pid")
    procs_at_map = descendants(p.pid)
    sched_at_map = sum(sched_seconds(q) for q in procs_at_map)
    cpu_at_map = sum(cpu_seconds(q) for q in procs_at_map)
    time.sleep(SETTLE)
    procs = descendants(p.pid)
    mem = {}
    for q in procs:
        for k, v in rollup(q).items():
            mem[k] = mem.get(k, 0) + v
    libs, gpu = set(), set()
    for q in procs:
        l, g = mapped(q)
        libs |= l
        gpu |= g
    cpu0 = sum(cpu_seconds(q) for q in procs)
    sc0 = sum(sched_seconds(q) for q in procs)
    sw0 = sum(switches(q) for q in procs)
    ti0 = now()
    time.sleep(idle)
    procs = descendants(p.pid)
    cpu1 = sum(cpu_seconds(q) for q in procs)
    sc1 = sum(sched_seconds(q) for q in procs)
    sw1 = sum(switches(q) for q in procs)
    ti1 = now()
    result = {
        "name": name,
        "first_frame_ms": round((t_map - t0) * 1000, 2),
        "cpu_to_first_frame_ms": round(cpu_at_map * 1000, 1),
        "cpu_to_first_frame_sched_ms": round(sched_at_map * 1000, 2),
        "window": {"app_id": c.get("app_id"), "pid": win_pid, "rect": c.get("rect"), "geometry": c.get("geometry")},
        "processes": len(procs),
        "threads": sum(threads(q) for q in procs),
        "pss_kib": mem.get("Pss", 0),
        "rss_kib": mem.get("Rss", 0),
        "pss_anon_kib": mem.get("Pss_Anon", 0),
        "pss_file_kib": mem.get("Pss_File", 0),
        "pss_shmem_kib": mem.get("Pss_Shmem", 0),
        "shared_objects": len(libs),
        "gpu_driver_libs": sorted(gpu),
        "idle_s": round(ti1 - ti0, 3),
        "idle_cpu_ms_per_s": round((cpu1 - cpu0) * 1000 / (ti1 - ti0), 3),
        "idle_cpu_sched_ms_per_s": round((sc1 - sc0) * 1000 / (ti1 - ti0), 4),
        "idle_switches_per_s": round((sw1 - sw0) / (ti1 - ti0), 2),
    }
    os.killpg(p.pid, signal.SIGTERM)
    try:
        p.wait(5)
    except subprocess.TimeoutExpired:
        os.killpg(p.pid, signal.SIGKILL)
        p.wait()
    time.sleep(0.5)
    return result


def main():
    args = sys.argv[1:]
    out = args.pop(0)
    reps, idle = 10, 20.0
    programs = []
    while args:
        a = args.pop(0)
        if a == "--reps":
            reps = int(args.pop(0))
        elif a == "--idle":
            idle = float(args.pop(0))
        else:
            name, cmd = a.split("=", 1)
            programs.append((name, shlex.split(cmd)))
    tmp = tempfile.mkdtemp(prefix="wc.", dir=os.environ.get("TMPDIR") or "/tmp")
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
    sway = subprocess.Popen(["sway", "-c", conf], env=env, stdout=subprocess.DEVNULL, stderr=open(f"{tmp}/sway.log", "w"))
    for _ in range(100):
        socks = [s for s in os.listdir(tmp) if s.startswith("sway-ipc")]
        ways = [s for s in os.listdir(tmp) if s.startswith("wayland-") and not s.endswith(".lock")]
        if socks and ways:
            break
        time.sleep(0.1)
    env["SWAYSOCK"] = f"{tmp}/{socks[0]}"
    env["WAYLAND_DISPLAY"] = ways[0]
    bus = subprocess.Popen(["dbus-daemon", "--session", "--nofork", "--nopidfile", f"--address=unix:path={tmp}/bus",
                            "--print-address"], env=env, stdout=subprocess.PIPE, text=True)
    env["DBUS_SESSION_BUS_ADDRESS"] = bus.stdout.readline().strip()
    time.sleep(0.5)
    events = Events(env)
    time.sleep(0.3)
    record = {
        "date": time.strftime("%Y-%m-%dT%H:%M:%S%z"),
        "host": os.uname().nodename, "kernel": os.uname().release,
        "load_before": open("/proc/loadavg").read().split()[:3],
        "settle_s": SETTLE, "idle_s": idle, "reps": reps,
        "programs": {n: c for n, c in programs},
        "sway": subprocess.run(["sway", "--version"], capture_output=True, text=True).stdout.strip(),
        "render_node": env["WLR_RENDER_DRM_DEVICE"],
        "runs": [],
    }
    try:
        for name, cmd in programs:
            r = run_one(name, cmd, env, events, 2.0)
            r["warmup"] = True
            record["runs"].append(r)
        for i in range(reps):
            order = programs if i % 2 == 0 else list(reversed(programs))
            for name, cmd in order:
                r = run_one(name, cmd, env, events, idle)
                r["rep"] = i
                record["runs"].append(r)
                print(json.dumps(r), flush=True)
    finally:
        record["load_after"] = open("/proc/loadavg").read().split()[:3]
        events.proc.kill()
        bus.terminate()
        sway.terminate()
        sway.wait()
        with open(out, "w") as f:
            json.dump(record, f, indent=1)


if __name__ == "__main__":
    main()
