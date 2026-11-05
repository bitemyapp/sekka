#!/usr/bin/env python3
"""Energy and CPU cost of media players under a private headless compositor.

Each trial starts a fresh private session (D-Bus, compositor, shell), starts
one player on one file full screen (or nothing, for the idle baseline), lets
it settle, then measures a fixed window: socket power from the AMD PPT
sensor (the shared `powermeter`), the CPU time and context switches of the
player and of every other process of the session, the iGPU's busy
percentage and clock, per-process GPU engine time from DRM fdinfo, the
player's memory, and its own frame counters. Trials of a plan are run in
rounds, the subjects of a round in a shuffled order.

    bench.py --plan PLAN.json --out DIR [--rounds N] [--only SUBSTR]

The plan names clips and subjects (see plans/). Everything it runs comes from
environment variables set by run.sh (store paths); see docs/performance/media.md.
"""

import argparse
import json
import lzma
import os
import random
import re
import shutil
import signal
import socket
import subprocess
import sys
import threading
import time

PERF = os.environ.get("PERF_DIR", f"{os.environ.get('TMPDIR') or '/tmp'}/aizome-perf")
POWERMETER = os.environ.get("POWERMETER", f"{PERF}/bin/powermeter")
BASE = os.environ.get("BENCH_BASE", "/tmp/aizome-pm")  # short: sockets live under it
RUN = f"{BASE}/run"
HOME = f"{BASE}/home"
CLIPS = os.environ.get("CLIPS", f"{PERF}/media/clips")
HERE = os.path.dirname(os.path.abspath(__file__))

AIZOME = os.environ["AIZOME"]  # the aizome package (bin/aizome, aizomectl, aizome-shell, aizome-utsushi)
MPV = os.environ["MPV"]
VLC = os.environ["VLC"]
SWAY = os.environ.get("SWAY", "")
SATELLITE = os.environ["XWAYLAND_SATELLITE"]
DBUS = os.environ["DBUS"]
PIPEWIRE = os.environ["PIPEWIRE"]
TATAMI_MPV_CONF = os.environ["TATAMI_MPV_CONF"]

CARD = os.environ.get("IGPU_CARD", "/sys/class/drm/card1/device")
RENDER_NODE = os.environ.get("RENDER_NODE", "/dev/dri/renderD128")
NULL_SINK = "perf-null"
USER_RUNTIME = f"/run/user/{os.getuid()}"
X_DISPLAY = ":17"
CLK = os.sysconf("SC_CLK_TCK")


def log(*a):
    print(time.strftime("%H:%M:%S"), *a, file=sys.stderr, flush=True)


# ---------------------------------------------------------------- /proc


def read(path, default=""):
    try:
        with open(path) as f:
            return f.read()
    except OSError:
        return default


def proc_cpu(pid):
    """utime+stime of the whole thread group (dead threads included), s."""
    s = read(f"/proc/{pid}/stat")
    if not s:
        return None
    f = s[s.rindex(")") + 2 :].split()
    return (int(f[11]) + int(f[12])) / CLK


def thread_switches(pid):
    """{tid: (voluntary, involuntary)} for the live threads of pid."""
    out = {}
    try:
        tids = os.listdir(f"/proc/{pid}/task")
    except OSError:
        return out
    for t in tids:
        st = read(f"/proc/{pid}/task/{t}/status")
        v = re.search(r"^voluntary_ctxt_switches:\s+(\d+)", st, re.M)
        n = re.search(r"^nonvoluntary_ctxt_switches:\s+(\d+)", st, re.M)
        sched = read(f"/proc/{pid}/task/{t}/schedstat").split()
        run_ns = int(sched[0]) if sched else 0
        if v and n:
            out[f"{pid}/{t}"] = (int(v.group(1)), int(n.group(1)), run_ns)
    return out


def thread_names(pid):
    out = {}
    try:
        for t in os.listdir(f"/proc/{pid}/task"):
            out[t] = read(f"/proc/{pid}/task/{t}/comm").strip()
    except OSError:
        pass
    return out


def children_of(pid):
    kids = []
    for t in os.listdir(f"/proc/{pid}/task") if os.path.isdir(f"/proc/{pid}/task") else []:
        c = read(f"/proc/{pid}/task/{t}/children").split()
        kids += [int(x) for x in c]
    return kids


def tree(pid):
    out, todo = [], [pid]
    while todo:
        p = todo.pop()
        if p in out or not os.path.isdir(f"/proc/{p}"):
            continue
        out.append(p)
        todo += children_of(p)
    return out


def comm(pid):
    return read(f"/proc/{pid}/comm").strip()


def exe(pid):
    try:
        return os.readlink(f"/proc/{pid}/exe")
    except OSError:
        return ""


def memory(pid):
    m = {}
    for line in read(f"/proc/{pid}/smaps_rollup").splitlines():
        k, _, v = line.partition(":")
        if k in ("Rss", "Pss", "Pss_Anon", "Pss_File", "Pss_Shmem", "Swap"):
            m[k.lower() + "_kib"] = int(v.split()[0])
    return m


def drm_info(pid):
    """DRM fds of pid: the devices, and engine time per DRM client."""
    devices, clients, nvidia = set(), {}, 0
    try:
        fds = os.listdir(f"/proc/{pid}/fd")
    except OSError:
        return {"devices": [], "clients": {}, "nvidia_fds": 0}
    for fd in fds:
        try:
            target = os.readlink(f"/proc/{pid}/fd/{fd}")
        except OSError:
            continue
        if target.startswith("/dev/nvidia"):
            nvidia += 1
            devices.add(target)
            continue
        if not target.startswith("/dev/dri/"):
            continue
        devices.add(target)
        info = {}
        for line in read(f"/proc/{pid}/fdinfo/{fd}").splitlines():
            k, _, v = line.partition(":")
            info[k.strip()] = v.strip()
        cid = info.get("drm-client-id")
        if cid is None:
            continue
        rec = {"pdev": info.get("drm-pdev"), "driver": info.get("drm-driver")}
        for k, v in info.items():
            if k.startswith("drm-engine-") and v.endswith(" ns"):
                rec[k[len("drm-engine-") :]] = int(v.split()[0])
            elif k.startswith("drm-memory-") or k.startswith("drm-total-") or k.startswith("drm-resident-"):
                rec[k[4:]] = v
        clients[f"{info.get('drm-pdev')}#{cid}"] = rec
    return {"devices": sorted(devices), "clients": clients, "nvidia_fds": nvidia}


def system_busy():
    f = read("/proc/stat").splitlines()[0].split()[1:9]
    v = [int(x) for x in f]
    return (v[0] + v[1] + v[2] + v[5] + v[6] + v[7]) / CLK, (v[3] + v[4]) / CLK


def wait_quiet(limit=1.0, timeout=90):
    """Wait until the machine's other CPU use is below `limit` cores over 2 s."""
    t0 = time.monotonic()
    while True:
        b0, _ = system_busy()
        time.sleep(2)
        b1, _ = system_busy()
        busy = (b1 - b0) / 2
        if busy < limit or time.monotonic() - t0 > timeout:
            return {"busy_cores_before": round(busy, 3), "waited_s": round(time.monotonic() - t0, 1),
                    "loadavg": read("/proc/loadavg").split()[:3]}


# ---------------------------------------------------------------- sampling


class Sampler(threading.Thread):
    """Samples the iGPU (busy %, sclk) at 10 Hz and the threads of the
    tracked processes at 1 Hz (so threads that end mid-window keep their
    switches)."""

    def __init__(self, pids_fn):
        super().__init__(daemon=True)
        self.pids_fn = pids_fn
        self.stop = threading.Event()
        self.gpu = []
        self.threads = {}  # role -> {tid: last}
        self.first = {}
        busy = f"{CARD}/gpu_busy_percent"
        self.busy_path = busy if os.path.exists(busy) else None
        hw = [d for d in os.listdir(f"{CARD}/hwmon")] if os.path.isdir(f"{CARD}/hwmon") else []
        self.sclk_path = f"{CARD}/hwmon/{hw[0]}/freq1_input" if hw else None
        # The processor's temperature (k10temp Tctl), once a second: leakage
        # makes the idle floor drift with it.
        self.tctl_path = None
        for h in os.listdir("/sys/class/hwmon"):
            if read(f"/sys/class/hwmon/{h}/name").strip() == "k10temp":
                self.tctl_path = f"/sys/class/hwmon/{h}/temp1_input"
        self.tctl = []

    def snap_threads(self):
        for role, pids in self.pids_fn().items():
            cur = {}
            for p in pids:
                cur.update(thread_switches(p))
            seen = self.threads.setdefault(role, {})
            first = self.first.setdefault(role, None)
            if first is None:
                self.first[role] = dict(cur)
            seen.update(cur)

    def run(self):
        self.snap_threads()
        t0 = time.monotonic()
        n = 0
        while not self.stop.is_set():
            n += 1
            nxt = t0 + n * 0.1
            b = read(self.busy_path).strip() if self.busy_path else ""
            s = read(self.sclk_path).strip() if self.sclk_path else ""
            self.gpu.append((round(time.monotonic() - t0, 3), int(b) if b else -1, int(s) // 1000000 if s else -1))
            if n % 10 == 0:
                self.snap_threads()
                if self.tctl_path:
                    t = read(self.tctl_path).strip()
                    if t:
                        self.tctl.append(int(t) / 1000)
            d = nxt - time.monotonic()
            if d > 0:
                self.stop.wait(d)
        self.snap_threads()

    def switches(self, role):
        """Voluntary and involuntary switches and CPU time (ns, from
        schedstat) of the role's threads over the window."""
        first = self.first.get(role) or {}
        last = self.threads.get(role) or {}
        vol = inv = run = 0
        for tid, (v, n, r) in last.items():
            v0, n0, r0 = first.get(tid, (0, 0, 0))
            vol += v - v0
            inv += n - n0
            run += r - r0
        return vol, inv, run


# ---------------------------------------------------------------- session


def run_cmd(args, env=None, timeout=10):
    return subprocess.run(args, env=env, capture_output=True, text=True, timeout=timeout)


def fresh_dirs():
    for d in (RUN, HOME):
        shutil.rmtree(d, ignore_errors=True)
    os.makedirs(RUN, mode=0o700)
    for d in (".config", ".local/state", ".local/share", ".cache"):
        os.makedirs(f"{HOME}/{d}")
    os.chmod(RUN, 0o700)


def base_env():
    env = {k: v for k, v in os.environ.items() if k not in (
        "WAYLAND_DISPLAY", "DISPLAY", "WAYLAND_SOCKET", "DBUS_SESSION_BUS_ADDRESS", "AIZOME_SOCKET",
        "AIZOME_SHELL_SOCKET", "SWAYSOCK", "I3SOCK", "PIPEWIRE_RUNTIME_DIR", "PULSE_SERVER", "PIPEWIRE_REMOTE",
        "XDG_SESSION_ID", "XDG_SEAT", "XDG_VTNR", "XDG_CURRENT_DESKTOP", "XDG_SESSION_DESKTOP", "XDG_SESSION_TYPE",
        "NOTIFY_SOCKET", "INVOCATION_ID", "JOURNAL_STREAM", "MANAGERPID", "SYSTEMD_EXEC_PID")}
    env.update({
        "XDG_RUNTIME_DIR": RUN,
        "HOME": HOME,
        "XDG_CONFIG_HOME": f"{HOME}/.config",
        "XDG_STATE_HOME": f"{HOME}/.local/state",
        "XDG_DATA_HOME": f"{HOME}/.local/share",
        "XDG_CACHE_HOME": f"{HOME}/.cache",
        "DBUS_SESSION_BUS_ADDRESS": f"unix:path={RUN}/bus",
        # The AMD iGPU for everything: GL, Vulkan, VA-API, VDPAU.
        "__EGL_VENDOR_LIBRARY_FILENAMES": "/run/opengl-driver/share/glvnd/egl_vendor.d/50_mesa.json",
        "__GLX_VENDOR_LIBRARY_NAME": "mesa",
        "VK_DRIVER_FILES": "/run/opengl-driver/share/vulkan/icd.d/radeon_icd.x86_64.json",
        "LIBVA_DRIVER_NAME": "radeonsi",
        "VDPAU_DRIVER": "radeonsi",
        # No CUDA devices: GStreamer's nvcodec (Showtime) would decode on the
        # NVIDIA card, outside the socket sensor.
        "CUDA_VISIBLE_DEVICES": "",
        "AIZOME_RENDER_NODE": RENDER_NODE,
        "WLR_RENDER_DRM_DEVICE": RENDER_NODE,
        "AIZOME_LOG": "warn",
        "PATH": ":".join([f"{AIZOME}/bin", f"{DBUS}/bin", os.environ.get("PATH", "")]),
    })
    return env


class Session:
    """A private session: D-Bus, a headless compositor (Aizome with its
    shell, or sway), and Xwayland (xwayland-satellite) when asked."""

    def __init__(self, kind, logdir, xwayland=False):
        self.kind = kind
        self.logdir = logdir
        self.xwayland = xwayland
        self.procs = {}
        self.env = base_env()

    def start(self):
        fresh_dirs()
        env = self.env
        with open(f"{RUN}/bus.conf", "w") as f:
            f.write(BUS_CONF.format(path=f"{RUN}/bus"))
        self.procs["dbus"] = subprocess.Popen(
            [f"{DBUS}/bin/dbus-daemon", f"--config-file={RUN}/bus.conf", "--nofork", "--nopidfile"],
            env=env, stdout=subprocess.DEVNULL, stderr=open(f"{self.logdir}/dbus.log", "w"))
        wait_for(lambda: os.path.exists(f"{RUN}/bus"), 5, "dbus socket")
        clog = open(f"{self.logdir}/compositor.log", "w")
        if self.kind == "aizome":
            self.procs["compositor"] = subprocess.Popen(
                [f"{AIZOME}/bin/aizome", "--backend", "headless", "--headless-output", "1920x1080@1",
                 "--socket", "az", "--no-xwayland", "--no-session"],
                env=env, stdout=clog, stderr=subprocess.STDOUT, start_new_session=True)
            env["WAYLAND_DISPLAY"] = "az"
            env["AIZOME_SOCKET"] = f"{RUN}/aizome.az.sock"
            env["AIZOME_SHELL_SOCKET"] = f"{RUN}/aizome-shell.az.sock"
            wait_for(lambda: os.path.exists(env["AIZOME_SOCKET"]), 10, "compositor IPC")
            wait_for(lambda: os.path.exists(env["AIZOME_SHELL_SOCKET"]), 15, "shell socket")
            wait_for(lambda: self.shell_pid() is not None, 5, "shell process")
        else:
            cfg = f"{RUN}/sway.conf"
            with open(cfg, "w") as f:
                f.write(SWAY_CONF)
            senv = dict(env)
            senv.update({"WLR_BACKENDS": "headless", "WLR_RENDERER": "gles2", "WLR_HEADLESS_OUTPUTS": "1",
                         "WLR_LIBINPUT_NO_DEVICES": "1", "SWAYSOCK": f"{RUN}/sway.sock"})
            self.procs["compositor"] = subprocess.Popen(
                [f"{SWAY}/bin/sway", "-c", cfg], env=senv, stdout=clog, stderr=subprocess.STDOUT,
                start_new_session=True)
            env["SWAYSOCK"] = f"{RUN}/sway.sock"
            wait_for(lambda: any(n.startswith("wayland-") and not n.endswith(".lock") for n in os.listdir(RUN)),
                     10, "sway socket")
            env["WAYLAND_DISPLAY"] = [n for n in os.listdir(RUN) if n.startswith("wayland-") and not n.endswith(".lock")][0]
            wait_for(lambda: os.path.exists(env["SWAYSOCK"]), 10, "sway IPC")
        if self.xwayland:
            if self.kind == "aizome":
                self.procs["xwayland"] = subprocess.Popen(
                    [f"{SATELLITE}/bin/xwayland-satellite", X_DISPLAY], env=env,
                    stdout=open(f"{self.logdir}/xwayland.log", "w"), stderr=subprocess.STDOUT,
                    start_new_session=True)
                wait_for(lambda: os.path.exists(f"/tmp/.X11-unix/X{X_DISPLAY[1:]}"), 10, "X socket")
                env["DISPLAY"] = X_DISPLAY
            else:
                # sway's own Xwayland, started when the first X client connects.
                env["DISPLAY"] = self.sway_display()
        time.sleep(0.5)

    def sway_display(self):
        """The X display sway exports to its children (its Xwayland starts
        when the first X client connects)."""
        f = f"{RUN}/sway-display"
        self.swaymsg("exec", f"echo $DISPLAY > {f}")
        wait_for(lambda: os.path.exists(f) and read(f).strip(), 5, "sway DISPLAY")
        d = read(f).strip()
        if not d.startswith(":"):
            raise RuntimeError("sway has no DISPLAY (xwayland disabled?)")
        return d

    def shell_pid(self):
        if self.kind != "aizome":
            return None
        for c in children_of(self.procs["compositor"].pid):
            if comm(c).startswith("aizome-shell"):
                return c
        return None

    def roles(self):
        """Processes of the session other than the player, by role."""
        r = {"compositor": [self.procs["compositor"].pid]}
        if self.kind == "aizome":
            s = self.shell_pid()
            r["shell"] = [s] if s else []
            # Anything else the compositor started (none expected).
        if "xwayland" in self.procs:
            r["xwayland"] = tree(self.procs["xwayland"].pid)
        elif self.kind == "sway" and self.xwayland:
            r["xwayland"] = [c for c in children_of(self.procs["compositor"].pid) if comm(c) == "Xwayland"]
        r["dbus"] = [self.procs["dbus"].pid]
        return r

    def ctl(self, *args, timeout=10):
        r = run_cmd([f"{AIZOME}/bin/aizomectl", "--json", *args], self.env, timeout)
        if r.returncode != 0:
            raise RuntimeError(f"aizomectl {args}: {r.stderr.strip()}")
        return json.loads(r.stdout) if r.stdout.strip() else None

    def swaymsg(self, *args):
        r = run_cmd([f"{SWAY}/bin/swaymsg", "-s", self.env["SWAYSOCK"], *args], self.env)
        return json.loads(r.stdout) if r.stdout.strip() else None

    def windows(self):
        if self.kind == "aizome":
            return [{"id": w["id"], "app_id": w["app_id"], "pid": w["pid"], "fullscreen": w["fullscreen"],
                     "geometry": w["geometry"], "title": w["title"]} for w in self.ctl("windows")["windows"]]
        out = []

        def walk(n):
            if n.get("type") in ("con", "floating_con") and n.get("pid"):
                r = n["rect"]
                out.append({"id": n["id"], "app_id": n.get("app_id") or (n.get("window_properties") or {}).get("class"),
                            "pid": n["pid"], "fullscreen": n.get("fullscreen_mode", 0) == 1,
                            "geometry": {"x": r["x"], "y": r["y"], "w": r["width"], "h": r["height"]},
                            "title": n.get("name")})
            for c in n.get("nodes", []) + n.get("floating_nodes", []):
                walk(c)

        walk(self.swaymsg("-t", "get_tree"))
        return out

    def fullscreen(self, wid):
        if self.kind == "aizome":
            self.ctl("fullscreen", str(wid), "on")
        else:
            self.swaymsg(f"[con_id={wid}] fullscreen enable")

    def comp_stats(self, reset=False):
        if self.kind != "aizome":
            return None
        try:
            s = self.ctl("stats", *(["--reset"] if reset else []))["stats"]
            return [{k: o[k] for k in ("name", "frames_rendered", "frames_skipped", "cpu_us", "gpu_us", "samples")
                     if k in o} for o in s["outputs"]]
        except Exception as e:  # noqa: BLE001
            return {"error": str(e)}

    def stop(self):
        for name in ("xwayland", "compositor", "dbus"):
            p = self.procs.get(name)
            if p and p.poll() is None:
                try:
                    os.killpg(p.pid, signal.SIGTERM) if name != "dbus" else p.terminate()
                except ProcessLookupError:
                    pass
                try:
                    p.wait(5)
                except subprocess.TimeoutExpired:
                    try:
                        os.killpg(p.pid, signal.SIGKILL) if name != "dbus" else p.kill()
                    except ProcessLookupError:
                        pass
                    p.wait(5)
        if self.xwayland and self.kind == "aizome":
            for f in (f"/tmp/.X11-unix/X{X_DISPLAY[1:]}", f"/tmp/.X{X_DISPLAY[1:]}-lock"):
                try:
                    os.unlink(f)
                except OSError:
                    pass


BUS_CONF = """<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <type>session</type>
  <listen>unix:path={path}</listen>
  <auth>EXTERNAL</auth>
  <policy context="default">
    <allow send_destination="*" eavesdrop="true"/>
    <allow eavesdrop="true"/>
    <allow own="*"/>
  </policy>
</busconfig>
"""

SWAY_CONF = """output HEADLESS-1 resolution 1920x1080 scale 1
default_border none
default_floating_border none
focus_follows_mouse no
bar {
  mode invisible
}
"""


def wait_for(fn, timeout, what):
    t0 = time.monotonic()
    while time.monotonic() - t0 < timeout:
        try:
            if fn():
                return time.monotonic() - t0
        except Exception:  # noqa: BLE001
            pass
        time.sleep(0.05)
    raise RuntimeError(f"timed out waiting for {what}")


# ---------------------------------------------------------------- players


class Player:
    name = "?"
    needs_x = False
    has_window = True

    def __init__(self, subject, clip, sound, logdir):
        self.subject = subject
        self.clip = clip
        self.sound = sound
        self.logdir = logdir
        self.proc = None

    def env(self, base):
        e = dict(base)
        if self.sound:
            # The user's PipeWire, to the benchmark's null sink only.
            e["PIPEWIRE_RUNTIME_DIR"] = USER_RUNTIME
            e["PULSE_SERVER"] = f"unix:{USER_RUNTIME}/pulse/native"
            e["PIPEWIRE_NODE"] = NULL_SINK
            e["PULSE_SINK"] = NULL_SINK
        return e

    def start(self, env):
        self.out = open(f"{self.logdir}/player.log", "w")
        self.proc = subprocess.Popen(self.argv(), env=self.env(env), stdout=self.out, stderr=subprocess.STDOUT,
                                     cwd=HOME, start_new_session=True)
        return self.proc.pid

    def pid(self):
        return self.proc.pid

    def stats(self):
        return None

    def pause(self, session):
        raise NotImplementedError

    def stop(self):
        if self.proc and self.proc.poll() is None:
            try:
                os.killpg(self.proc.pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
            try:
                self.proc.wait(5)
            except subprocess.TimeoutExpired:
                os.killpg(self.proc.pid, signal.SIGKILL)
                self.proc.wait(5)


class Utsushi(Player):
    name = "utsushi"

    def argv(self):
        a = [f"{AIZOME}/bin/aizome-utsushi"]
        if not self.sound:
            a.append("--no-sound")
        return a + [self.clip]

    def env(self, base):
        e = super().env(base)
        e["UTSUSHI_STATS"] = "1"
        return e

    def stats(self):
        """The last UTSUSHI_STATS line (every 5 s)."""
        try:
            with open(f"{self.logdir}/player.log") as f:
                lines = [l for l in f if l.startswith("utsushi: Pipeline")]
        except OSError:
            return None
        if not lines:
            return None
        l = lines[-1]
        d = {k: int(v) for k, v in re.findall(r"\b(shown|dropped|decoded|presented|discarded|suspensions): (\d+)", l)}
        d["path"] = (re.search(r"path: (\w+)", l) or [None, None])[1]
        d["video_decoder"] = (re.search(r'video_decoder: Some\("([^"]+)"\)', l) or [None, None])[1]
        d["audio_output"] = (re.search(r'audio_output: "([^"]+)"', l) or [None, None])[1]
        d["gpu"] = "gpu: true" in l
        d["position"] = float(re.search(r" at ([\d.]+)\s*$", l).group(1))
        d["lines"] = len(lines)
        return d

    def pause(self, session):
        r = run_cmd([f"{DBUS}/bin/dbus-send", "--session", "--print-reply", "--dest=org.mpris.MediaPlayer2.utsushi",
                     "/org/mpris/MediaPlayer2", "org.mpris.MediaPlayer2.Player.Pause"], session.env)
        if r.returncode != 0:
            raise RuntimeError(f"MPRIS pause: {r.stderr.strip()}")


class Mpv(Player):
    name = "mpv"
    VARIANTS = {
        # mpv's own defaults: software decoding, gpu-next (Vulkan here).
        "mpv-default": ["--no-config"],
        # Sekka's /etc/mpv/mpv.conf (desktop/tatami/mpv.conf): OpenGL first.
        "mpv-sekka": ["--no-config", f"--include={TATAMI_MPV_CONF}"],
        "mpv-vaapi-gpunext": ["--no-config", "--hwdec=vaapi", "--vo=gpu-next"],
        "mpv-vaapi-dmabuf": ["--no-config", "--hwdec=vaapi", "--vo=dmabuf-wayland"],
    }

    def __init__(self, *a, **k):
        super().__init__(*a, **k)
        self.has_window = not self.clip_is_audio()

    def clip_is_audio(self):
        return self.clip.endswith((".mp3", ".opus", ".flac"))

    def argv(self):
        a = [f"{MPV}/bin/mpv", *self.VARIANTS[self.subject], f"--input-ipc-server={RUN}/mpv.sock"]
        if not self.clip_is_audio():
            a.append("--fs")
        if self.sound:
            a += ["--ao=pipewire", f"--audio-device=pipewire/{NULL_SINK}"]
        else:
            a.append("--ao=null")
        return a + [self.clip]

    def ipc(self, *cmd):
        s = socket.socket(socket.AF_UNIX)
        s.settimeout(2)
        s.connect(f"{RUN}/mpv.sock")
        s.sendall((json.dumps({"command": list(cmd)}) + "\n").encode())
        buf = b""
        while b"\n" not in buf:
            buf += s.recv(65536)
        s.close()
        for line in buf.split(b"\n"):
            if line.strip():
                m = json.loads(line)
                if "error" in m:
                    return m.get("data") if m["error"] == "success" else None
        return None

    PROPS = ["frame-drop-count", "decoder-frame-drop-count", "vo-delayed-frame-count", "mistimed-frame-count",
             "playback-time", "estimated-vf-fps", "hwdec-current", "current-vo", "current-gpu-context",
             "video-params/pixelformat", "video-params/hw-pixelformat", "video-out-params/w", "video-out-params/h",
             "display-fps", "current-ao", "audio-params/samplerate", "pause", "fullscreen"]

    def stats(self):
        try:
            return {p: self.ipc("get_property", p) for p in self.PROPS}
        except OSError as e:
            return {"error": str(e)}

    def pause(self, session):
        self.ipc("set_property", "pause", True)


class Vlc(Player):
    name = "vlc"
    needs_x = True

    def __init__(self, *a, **k):
        super().__init__(*a, **k)

    def argv(self):
        a = [f"{VLC}/bin/vlc", "--no-qt-privacy-ask", "--extraintf=oldrc", f"--rc-unix={RUN}/vlc.sock",
             "--rc-fake-tty"]
        if not self.clip.endswith((".mp3", ".opus", ".flac")):
            a.append("--fullscreen")
        if not self.sound:
            a.append("--aout=dummy")
        if self.subject == "vlc-swdec":
            a.append("--avcodec-hw=none")
        return a + [self.clip]

    def rc(self, cmd, wait=0.4):
        s = socket.socket(socket.AF_UNIX)
        s.settimeout(2)
        s.connect(f"{RUN}/vlc.sock")
        s.sendall((cmd + "\n").encode())
        time.sleep(wait)
        s.settimeout(0.3)
        buf = b""
        try:
            while True:
                b = s.recv(65536)
                if not b:
                    break
                buf += b
        except OSError:
            pass
        s.close()
        return buf.decode(errors="replace")

    def stats(self):
        try:
            t = self.rc("stats")
        except OSError as e:
            return {"error": str(e)}
        d = {}
        for k, key in (("frames displayed", "displayed"), ("frames lost", "lost"), ("video decoded", "video_decoded"),
                       ("buffers played", "audio_played"), ("buffers lost", "audio_lost"),
                       ("audio decoded", "audio_decoded")):
            m = re.search(rf"{k}\s*:\s*(\d+)", t)
            if m:
                d[key] = int(m.group(1))
        try:
            g = self.rc("get_time", 0.2)
            m = re.search(r"(\d+)", g)
            d["position"] = int(m.group(1)) if m else None
        except OSError:
            pass
        return d

    def pause(self, session):
        self.rc("pause", 0.2)


class Desktop(Player):
    """GNOME's Showtime (GTK 4, GStreamer) and KDE's Haruna (Qt, libmpv):
    no null audio output of their own, so only with sound (the null sink),
    full screen through the compositor, counters not read."""

    def argv(self):
        if not self.sound:
            raise RuntimeError(f"{self.subject} runs only with sound (the null sink)")
        prog = {"showtime": os.environ.get("SHOWTIME", ""), "haruna": os.environ.get("HARUNA", "")}[self.subject]
        if not prog:
            raise RuntimeError(f"{self.subject}: not configured")
        return [f"{prog}/bin/{self.subject}", self.clip]


def make_player(subject, clip, sound, logdir):
    if subject == "utsushi":
        return Utsushi(subject, clip, sound, logdir)
    if subject in ("showtime", "haruna"):
        return Desktop(subject, clip, sound, logdir)
    if subject.startswith("mpv"):
        return Mpv(subject, clip, sound, logdir)
    if subject.startswith("vlc"):
        return Vlc(subject, clip, sound, logdir)
    raise ValueError(subject)


# ---------------------------------------------------------------- PipeWire


def pw(*args, timeout=10):
    env = dict(os.environ)
    env["PIPEWIRE_RUNTIME_DIR"] = USER_RUNTIME
    env["XDG_RUNTIME_DIR"] = USER_RUNTIME
    return run_cmd([f"{PIPEWIRE}/bin/{args[0]}", *args[1:]], env, timeout)


def pw_dump():
    r = pw("pw-dump")
    return json.loads(r.stdout) if r.returncode == 0 else []


def default_sink():
    r = pw("pw-metadata", "0", "default.audio.sink")
    m = re.search(r"value:'([^']*)'", r.stdout)
    return m.group(1) if m else None


def null_sink_id(dump=None):
    for o in dump or pw_dump():
        if o.get("type") == "PipeWire:Interface:Node" and (o.get("info", {}).get("props", {}).get("node.name") == NULL_SINK):
            return o["id"]
    return None


def create_null_sink():
    if null_sink_id() is not None:
        return default_sink()
    before = default_sink()
    props = ("{ factory.name=support.null-audio-sink node.name=" + NULL_SINK +
             " node.description=\"Benchmark null sink\" media.class=Audio/Sink object.linger=true"
             " audio.position=[ FL FR ] audio.rate=48000 priority.session=0 priority.driver=0"
             " monitor.channel-volumes=true }")
    r = pw("pw-cli", "create-node", "adapter", props)
    if r.returncode != 0:
        raise RuntimeError(f"pw-cli create-node: {r.stderr} {r.stdout}")
    wait_for(lambda: null_sink_id() is not None, 5, "null sink")
    time.sleep(1)
    after = default_sink()
    if before != after:
        remove_null_sink()
        raise RuntimeError(f"default sink changed ({before} -> {after}); removed the null sink")
    return before


def remove_null_sink():
    i = null_sink_id()
    if i is not None:
        pw("pw-cli", "destroy", str(i))


def pipewire_pids():
    out = {}
    for p in os.listdir("/proc"):
        if not p.isdigit():
            continue
        try:
            if os.stat(f"/proc/{p}").st_uid != os.getuid():
                continue
        except OSError:
            continue
        c = comm(p)
        if c in ("pipewire", "pipewire-pulse", "wireplumber"):
            # The user's session daemons only (not anything under our private runtime).
            out.setdefault(c, []).append(int(p))
    return out


def player_links(player_pids):
    """Where the player's streams go: the sinks its output ports link to."""
    dump = pw_dump()
    nodes = {o["id"]: o for o in dump if o.get("type") == "PipeWire:Interface:Node"}
    ports = {o["id"]: o for o in dump if o.get("type") == "PipeWire:Interface:Port"}
    clients = {o["id"]: o for o in dump if o.get("type") == "PipeWire:Interface:Client"}
    mine = set()
    for nid, n in nodes.items():
        props = n.get("info", {}).get("props", {})
        pid = props.get("application.process.id")
        cl = clients.get(props.get("client.id"), {}).get("info", {}).get("props", {})
        pid = pid or cl.get("application.process.id") or cl.get("pipewire.sec.pid")
        try:
            pid = int(pid)
        except (TypeError, ValueError):
            continue
        if pid in player_pids and props.get("media.class", "").startswith("Stream/Output"):
            mine.add(nid)
    targets = set()
    for o in dump:
        if o.get("type") != "PipeWire:Interface:Link":
            continue
        i = o.get("info", {})
        if i.get("output-node-id") in mine:
            t = nodes.get(i.get("input-node-id"), {}).get("info", {}).get("props", {}).get("node.name")
            targets.add(t)
    return sorted(mine), sorted(t for t in targets if t)


# ---------------------------------------------------------------- trial


def snapshot(roles):
    out = {}
    for role, pids in roles.items():
        cpu = 0.0
        for p in pids:
            c = proc_cpu(p)
            cpu += c or 0.0
        out[role] = {"cpu_s": cpu, "pids": pids}
    return out


def gpu_snapshot(roles):
    return {role: {str(p): drm_info(p) for p in pids} for role, pids in roles.items()}


def gpu_delta(a, b):
    out = {}
    for role in b:
        eng, devs, nv = {}, set(), 0
        for pid, info in b[role].items():
            devs.update(info["devices"])
            nv += info["nvidia_fds"]
            for cid, rec in info["clients"].items():
                before = a.get(role, {}).get(pid, {}).get("clients", {}).get(cid, {})
                for k, v in rec.items():
                    if isinstance(v, int):
                        eng[k] = eng.get(k, 0) + v - before.get(k, 0)
        pdevs = sorted({rec.get("pdev") for info in b[role].values() for rec in info["clients"].values()} - {None})
        out[role] = {"engine_ns": eng, "devices": sorted(devs), "pdevs": pdevs, "nvidia_fds": nv}
    return out


def run_trial(spec, outdir, seq):
    """One trial: spec = {subject, clip, mode (play|pause|audio), compositor, sound, window_s}."""
    subject = spec["subject"]
    clip = spec.get("clip")
    mode = spec.get("mode", "play")
    sound = spec.get("sound", False)
    window = spec.get("window_s", 60)
    settle = spec.get("settle_s", 5)
    kind = spec.get("compositor", "aizome")
    tag = f"{seq:03d}-{kind}-{mode}-{os.path.basename(clip).split('.')[0] if clip else 'none'}-{subject}"
    tdir = f"{outdir}/trials/{tag}"
    os.makedirs(tdir, exist_ok=True)
    rec = {"tag": tag, "spec": spec, "started": time.strftime("%Y-%m-%dT%H:%M:%S%z")}
    rec["quiet"] = wait_quiet()
    player = None
    needs_x = subject.startswith("vlc")
    session = Session(kind, tdir, xwayland=needs_x)
    try:
        session.start()
        rec["session_up_s"] = None
        time.sleep(spec.get("session_settle_s", 3))
        pw_pids = pipewire_pids() if sound else {}
        if subject != "baseline":
            player = make_player(subject, f"{CLIPS}/{clip}", sound, tdir)
            t_launch = time.monotonic()
            ppid = player.start(session.env)
            win = None
            if player.has_window:
                def find():
                    nonlocal win
                    pids = set(tree(ppid))
                    for w in session.windows():
                        if w["pid"] in pids or (needs_x and w["app_id"] and "vlc" in str(w["app_id"]).lower()):
                            if w["geometry"]["w"] > 200:
                                win = w
                                return True
                    return False
                rec["window_after_s"] = round(wait_for(find, 20, "player window"), 3)
                if subject in ("utsushi", "showtime", "haruna") and mode != "audio":
                    session.fullscreen(win["id"])
                elif kind == "sway" and not needs_x and not win["fullscreen"] and mode != "audio":
                    session.fullscreen(win["id"])
                if mode != "audio" and spec.get("ensure_fullscreen"):
                    # A player whose own flag didn't take (mpv's --fs with
                    # dmabuf-wayland under Aizome): the compositor does it.
                    time.sleep(1.0)
                    for w in session.windows():
                        if w["id"] == win["id"] and not w["fullscreen"]:
                            session.fullscreen(w["id"])
                            rec["fullscreen_by_compositor"] = True
            if sound:
                time.sleep(1.0)
                nodes, targets = player_links(set(tree(ppid)))
                rec["audio_route"] = {"nodes": nodes, "targets": targets}
                if any(t != NULL_SINK for t in targets):
                    raise RuntimeError(f"player audio not on the null sink: {targets}")
                if not targets:
                    rec["audio_route"]["warning"] = "no stream linked yet"
            time.sleep(settle)
            if mode == "pause":
                player.pause(session)
                time.sleep(spec.get("pause_settle_s", 3))
            if player.has_window:
                rec["windows"] = session.windows()
            if spec.get("shot"):
                # Smoke runs only: what the output shows (not during a window).
                if kind == "aizome":
                    session.ctl("screenshot", f"{tdir}/shot.png")
                elif os.environ.get("GRIM"):
                    run_cmd([f"{os.environ['GRIM']}/bin/grim", f"{tdir}/shot.png"], session.env)
            if sound:
                nodes, targets = player_links(set(tree(ppid)))
                rec["audio_route_before_window"] = {"nodes": nodes, "targets": targets}
                if any(t != NULL_SINK for t in targets):
                    raise RuntimeError(f"player audio not on the null sink: {targets}")
        roles = session.roles()
        if player:
            roles["player"] = tree(player.pid())
        for k, v in pw_pids.items():
            roles[f"pw:{k}"] = v
        rec["roles"] = {r: [{"pid": p, "comm": comm(p), "exe": exe(p)} for p in ps] for r, ps in roles.items()}
        rec["player_stats_before"] = player.stats() if player else None
        session.comp_stats(reset=True)
        g0 = gpu_snapshot(roles)
        s0 = snapshot(roles)
        sampler = Sampler(lambda: roles)
        sampler.start()
        pm = subprocess.run([POWERMETER, "-i", "10", "-o", f"{tdir}/power.tsv", "-j", f"{tdir}/power.json",
                             "--", "sleep", str(window)], capture_output=True, text=True)
        sampler.stop.set()
        sampler.join()
        s1 = snapshot(roles)
        g1 = gpu_snapshot(roles)
        rec["compositor_stats"] = session.comp_stats()
        rec["player_stats_after"] = player.stats() if player else None
        rec["power"] = json.loads(read(f"{tdir}/power.json") or "{}")
        dur = rec["power"].get("duration_s", window)
        rec["cpu"] = {}
        for role in roles:
            vol, inv, run = sampler.switches(role)
            rec["cpu"][role] = {
                # schedstat run time of the threads (ns precision); the
                # tick-based utime+stime of /proc/PID/stat as a check.
                "cpu_s": round(run / 1e9, 4),
                "cpu_pct": round(100 * run / 1e9 / dur, 3),
                "cpu_ticks_s": round(s1[role]["cpu_s"] - s0[role]["cpu_s"], 3),
                "vol_switches": vol, "invol_switches": inv,
                "switches_per_s": round((vol + inv) / dur, 1), "wakeups_per_s": round(vol / dur, 1)}
        rec["gpu"] = gpu_delta(g0, g1)
        busy = [b for _, b, _ in sampler.gpu if b >= 0]
        sclk = [s for _, _, s in sampler.gpu if s >= 0]
        rec["igpu"] = {"busy_pct_mean": round(sum(busy) / len(busy), 2) if busy else None,
                       "busy_samples": len(busy),
                       "sclk_mhz_mean": round(sum(sclk) / len(sclk), 1) if sclk else None,
                       "tctl_c_mean": round(sum(sampler.tctl) / len(sampler.tctl), 2) if sampler.tctl else None}
        with lzma.open(f"{tdir}/igpu.tsv.xz", "wt") as f:
            f.write("t_s\tbusy_pct\tsclk_mhz\n")
            for t, b, s in sampler.gpu:
                f.write(f"{t}\t{b}\t{s}\n")
        if player:
            mem = {}
            for p in roles["player"]:
                for k, v in memory(p).items():
                    mem[k] = mem.get(k, 0) + v
            rec["player_memory"] = mem
            rec["player_threads"] = {str(p): thread_names(p) for p in roles["player"]}
            rec["player_alive"] = player.proc.poll() is None
        rec["compositor_memory"] = memory(roles["compositor"][0])
        # Threads' switches per name, for the player (what wakes).
        per = {}
        first = sampler.first.get("player") or {}
        for tid, (v, n, r) in (sampler.threads.get("player") or {}).items():
            pid, t = tid.split("/")
            name = (rec.get("player_threads", {}).get(pid, {}) or {}).get(t, t)
            v0, n0, r0 = first.get(tid, (0, 0, 0))
            e = per.setdefault(name, [0, 0, 0.0])
            e[0] += v - v0
            e[1] += n - n0
            e[2] = round(e[2] + (r - r0) / 1e6, 1)  # ms on CPU
        rec["player_switches_by_thread"] = per
        with lzma.open(f"{tdir}/power.tsv.xz", "wt") as f:
            f.write(read(f"{tdir}/power.tsv"))
        os.unlink(f"{tdir}/power.tsv")
        rec["ok"] = True
    except Exception as e:  # noqa: BLE001
        rec["ok"] = False
        rec["error"] = f"{type(e).__name__}: {e}"
        log("trial failed", tag, rec["error"])
    finally:
        if player:
            player.stop()
        session.stop()
        rec["loadavg_after"] = read("/proc/loadavg").split()[:3]
        with open(f"{tdir}/trial.json", "w") as f:
            json.dump(rec, f, indent=1)
        # Logs: keep them small.
        for name in os.listdir(tdir):
            p = f"{tdir}/{name}"
            if name.endswith(".log"):
                if os.path.getsize(p) > 0:
                    with open(p, "rb") as src, lzma.open(p + ".xz", "wb") as dst:
                        dst.write(src.read()[-2_000_000:])
                os.unlink(p)
    return rec


def summary_line(rec):
    if not rec.get("ok"):
        return f"{rec['tag']}: FAILED {rec.get('error')}"
    p = rec["power"]
    c = rec["cpu"]
    pl = c.get("player", {})
    co = c.get("compositor", {})
    return (f"{rec['tag']}: {p.get('mean_w', 0):.2f} W, player {pl.get('cpu_pct', 0):.2f}% "
            f"{pl.get('switches_per_s', 0):.0f}/s, compositor {co.get('cpu_pct', 0):.2f}% "
            f"{co.get('switches_per_s', 0):.0f}/s, igpu {rec['igpu']['busy_pct_mean']}% "
            f"{rec['igpu']['sclk_mhz_mean']} MHz, sys {p.get('system_busy_cpu_s', 0):.1f} cpu-s")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--plan", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--rounds", type=int, default=None)
    ap.add_argument("--seed", type=int, default=None)
    ap.add_argument("--max-minutes", type=float, default=42)
    args = ap.parse_args()
    plan = json.load(open(args.plan))
    os.makedirs(f"{args.out}/trials", exist_ok=True)
    rounds = args.rounds or plan.get("rounds", 3)
    seed = args.seed if args.seed is not None else int(time.time())
    rng = random.Random(seed)
    existing = [d for d in os.listdir(f"{args.out}/trials")]
    seq = len(existing)
    t_start = time.monotonic()
    needs_sink = any(s.get("sound") for s in plan["trials"])
    sink_before = None
    if needs_sink:
        sink_before = create_null_sink()
        log("null sink up")
    u = os.uname()
    env_rec = {"seed": seed, "plan": plan, "kernel": u.release, "machine": u.machine,
               "started": time.strftime("%Y-%m-%dT%H:%M:%S%z"), "loadavg": read("/proc/loadavg").split()[:3]}
    try:
        for r in range(rounds):
            order = list(plan["trials"])
            rng.shuffle(order)
            for spec in order:
                if (time.monotonic() - t_start) / 60 > args.max_minutes:
                    log("time budget reached; stopping")
                    raise StopIteration
                seq += 1
                rec = run_trial(dict(spec, round=r), args.out, seq)
                log(summary_line(rec))
                with open(f"{args.out}/results.jsonl", "a") as f:
                    f.write(json.dumps({k: rec.get(k) for k in (
                        "tag", "spec", "ok", "error", "quiet", "power", "cpu", "igpu", "gpu", "player_memory",
                        "player_stats_before", "player_stats_after", "compositor_stats", "window_after_s",
                        "audio_route_before_window", "loadavg_after")}) + "\n")
                time.sleep(1)
    except StopIteration:
        pass
    finally:
        if needs_sink:
            remove_null_sink()
            env_rec["default_sink_unchanged"] = default_sink() == sink_before
            log("null sink removed; default sink unchanged:", env_rec["default_sink_unchanged"])
        env_rec["finished"] = time.strftime("%Y-%m-%dT%H:%M:%S%z")
        with open(f"{args.out}/batches.jsonl", "a") as f:
            f.write(json.dumps(env_rec) + "\n")


if __name__ == "__main__":
    main()
