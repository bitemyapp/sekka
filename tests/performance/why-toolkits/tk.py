#!/usr/bin/env python3
"""The why-toolkits harness's common parts (docs/performance/why-toolkits.md):
the subjects, a private headless sway, a private session bus, the
allowlisted environment, the tracing proxy, and what is read from /proc.

Everything a run starts is in a process group of its own and killed with it.
Paths are short (UNIX socket paths are limited to 108 bytes)."""
import json
import os
import random
import re
import shutil
import signal
import socket
import struct
import subprocess
import sys
import time

TK = os.environ.get("TK_WORK", "/tmp/claude-1000/tk")
ROOTS = f"{TK}/roots"
TOOLS = f"{TK}/tools"
FONTS = os.environ.get("TK_FONTS", "/tmp/claude-1000/aizome-fonts")
MESA_EGL = "/run/opengl-driver/share/glvnd/egl_vendor.d/50_mesa.json"
RADV = "/run/opengl-driver/share/vulkan/icd.d/radeon_icd.x86_64.json"
RENDER_NODE = "/dev/dri/renderD128"
APPS = f"{ROOTS}/apps/bin"
KIT = os.environ.get("TK_KIT", f"{TK}/bin/tk-kit")
AIZOME = f"{ROOTS}/aizome-flake/bin"
FIX = f"{TK}/fixtures"

# CPUs: the subject on 2-5, the compositor on 6 and 14, the observer and
# proxy on 7, the harness and power sampler on 15 (as the terminal harness).
CPU_APP = "2-5"
CPU_COMP = "6,14"
CPU_TOOLS = "7"
CPU_HARNESS = "15"
TASKSET = f"{ROOTS}/util-linux-bin/bin/taskset"


RESPONDER = [f"{ROOTS}/python3/bin/python3", "-I", os.path.join(os.path.dirname(os.path.abspath(__file__)), "responder.py")]


def subjects():
    """name -> (argv prefix, app_id, family)."""
    return {
        "kit": ([KIT], "org.tk.Kit", "kit"),
        "gtk4": ([f"{APPS}/tk-gtk4"], "org.tk.Gtk4", "gtk"),
        "adw": ([f"{APPS}/tk-adw"], "org.tk.Adw", "gtk"),
        "qtwidgets": ([f"{APPS}/tk-qtwidgets"], "org.tk.QtWidgets", "qt"),
        "qtquick": ([f"{APPS}/tk-qtquick"], "org.tk.QtQuick", "qt"),
        # Real apps.
        "sumi": ([f"{AIZOME}/aizome-sumi"], "org.aizome.Sumi", "kit"),
        "gte": ([f"{ROOTS}/gnome-text-editor/bin/gnome-text-editor", "--standalone"], "org.gnome.TextEditor", "gtk"),
        "kwrite": ([f"{ROOTS}/kate/bin/kwrite"], "org.kde.kwrite", "qt"),
        "hako": ([f"{AIZOME}/aizome-hako"], "org.aizome.Hako", "kit"),
        "nautilus": ([f"{ROOTS}/nautilus/bin/nautilus", "--new-window"], "org.gnome.Nautilus", "gtk"),
        "dolphin": ([f"{ROOTS}/dolphin/bin/dolphin", "--new-window"], "org.kde.dolphin", "qt"),
        # Terminals running cat (the tty echoes each key), as in terminals.md's
        # matched configuration: for Hotaru's extra frame.
        "hotaru-cat": ([f"{AIZOME}/aizome-hotaru", "--config-file", f"{TK}/term/hotaru.toml", "--", "cat"],
                       "org.aizome.Hotaru", "kit"),
        "foot-cat": ([f"{ROOTS}/foot/bin/foot", "--config", f"{TK}/term/foot.ini", "--", "cat"], "foot", "foot"),
        # A raw-mode program answering each key in one write, or in two.
        "hotaru": ([f"{AIZOME}/aizome-hotaru", "--config-file", f"{TK}/term/hotaru.toml", "--", *RESPONDER],
                   "org.aizome.Hotaru", "kit"),
        "foot": ([f"{ROOTS}/foot/bin/foot", "--config", f"{TK}/term/foot.ini", "--", *RESPONDER], "foot", "foot"),
        "hotaru-two": ([f"{AIZOME}/aizome-hotaru", "--config-file", f"{TK}/term/hotaru.toml", "--", *RESPONDER, "two"],
                       "org.aizome.Hotaru", "kit"),
        "foot-two": ([f"{ROOTS}/foot/bin/foot", "--config", f"{TK}/term/foot.ini", "--", *RESPONDER, "two"], "foot", "foot"),
    }


def terminal_configs():
    os.makedirs(f"{TK}/term", exist_ok=True)
    with open(f"{TK}/term/hotaru.toml", "w") as f:
        f.write('font_size = 14\nscrollback = 0\ntheme = "night"\nshell_integration = false\ncursor_blink = false\n')
    with open(f"{TK}/term/foot.ini", "w") as f:
        f.write("font=IBM Plex Mono:size=14\nline-height=24px\n\n[scrollback]\nlines=0\n\n[cursor]\nblink=no\n\n"
                "[colors-dark]\nalpha=1.0\n")


def now_ns():
    return time.clock_gettime_ns(time.CLOCK_MONOTONIC)


def loadavg():
    with open("/proc/loadavg") as f:
        return f.read().split()[:3]


def wait_quiet(limit=1.5, timeout=300):
    """Wait for a one-minute load average under `limit` (up to `timeout` s)."""
    t0 = time.monotonic()
    while time.monotonic() - t0 < timeout:
        if float(loadavg()[0]) < limit:
            return True
        time.sleep(5)
    return False


# ------------------------------------------------------------------ /proc


def descendants(pid):
    out, todo = [], [pid]
    while todo:
        p = todo.pop()
        out.append(p)
        try:
            for t in os.listdir(f"/proc/{p}/task"):
                with open(f"/proc/{p}/task/{t}/children") as f:
                    todo += [int(x) for x in f.read().split()]
        except OSError:
            pass
    return out


def comm(pid):
    try:
        with open(f"/proc/{pid}/comm") as f:
            return f.read().strip()
    except OSError:
        return "?"


def argv0(pid):
    """A process's program name: argv[0]'s base, Nix's wrapper prefix and
    suffix taken off."""
    try:
        with open(f"/proc/{pid}/cmdline", "rb") as f:
            a = f.read().split(b"\0")[0].decode(errors="replace")
    except OSError:
        return "?"
    return re.sub(r"-wrapped$", "", os.path.basename(a).lstrip("."))


def cpu(pids):
    """Run time (ns) and voluntary/involuntary switches, every thread of pids."""
    run = vol = inv = 0
    per = {}
    for p in pids:
        try:
            tids = os.listdir(f"/proc/{p}/task")
        except OSError:
            continue
        for t in tids:
            try:
                with open(f"/proc/{p}/task/{t}/schedstat") as f:
                    r = int(f.read().split()[0])
                with open(f"/proc/{p}/task/{t}/status") as f:
                    st = f.read()
                v = int(re.search(r"^voluntary_ctxt_switches:\s+(\d+)", st, re.M).group(1))
                n = int(re.search(r"^nonvoluntary_ctxt_switches:\s+(\d+)", st, re.M).group(1))
                with open(f"/proc/{p}/task/{t}/comm") as f:
                    c = f.read().strip()
            except (OSError, AttributeError, ValueError):
                continue
            run += r
            vol += v
            inv += n
            k = re.sub(r"[0-9]+$", "#", c)
            e = per.setdefault(k, [0, 0])
            e[0] += r
            e[1] += v
    return dict(run_ns=run, vol=vol, inv=inv, threads=per)


def gpu(pids):
    """amdgpu's per-client counters (fdinfo): gfx engine ns, memory kB."""
    seen = {}
    for p in pids:
        try:
            fds = os.listdir(f"/proc/{p}/fdinfo")
        except OSError:
            continue
        for fd in fds:
            try:
                with open(f"/proc/{p}/fdinfo/{fd}") as f:
                    txt = f.read()
            except OSError:
                continue
            if "drm-client-id" not in txt or "drm-driver:\tamdgpu" not in txt and "drm-driver: amdgpu" not in txt:
                continue
            cid = re.search(r"drm-client-id:\s+(\d+)", txt).group(1)
            eng = sum(int(x) for x in re.findall(r"drm-engine-(?:gfx|compute|dma)[^:]*:\s+(\d+) ns", txt))
            gfx = sum(int(x) for x in re.findall(r"drm-engine-gfx:\s+(\d+) ns", txt))
            mem = sum(int(x) for x in re.findall(r"drm-total-(?:vram|gtt):\s+(\d+) KiB", txt))
            seen[cid] = dict(engine_ns=eng, gfx_ns=gfx, mem_kib=mem)
    return dict(engine_ns=sum(v["engine_ns"] for v in seen.values()),
                gfx_ns=sum(v["gfx_ns"] for v in seen.values()),
                mem_kib=sum(v["mem_kib"] for v in seen.values()), clients=len(seen))


def sample(pids):
    return dict(t=now_ns(), cpu=cpu(pids), gpu=gpu(pids))


def diff(a, b):
    dt = (b["t"] - a["t"]) / 1e9
    return dict(seconds=dt,
                cpu_ms=(b["cpu"]["run_ns"] - a["cpu"]["run_ns"]) / 1e6,
                vol=b["cpu"]["vol"] - a["cpu"]["vol"],
                inv=b["cpu"]["inv"] - a["cpu"]["inv"],
                gpu_ms=(b["gpu"]["engine_ns"] - a["gpu"]["engine_ns"]) / 1e6,
                gpu_mem_kib=b["gpu"]["mem_kib"],
                threads={k: round((v[0] - a["cpu"]["threads"].get(k, [0, 0])[0]) / 1e6, 3)
                         for k, v in b["cpu"]["threads"].items()
                         if v[0] - a["cpu"]["threads"].get(k, [0, 0])[0] > 0})


def ppt_energy(tsv, t0, t1):
    """Joules and mean watts from the PPT sampler's lines over [t0, t1] (ns)."""
    pts = []
    try:
        with open(tsv) as f:
            for line in f:
                p = line.split()
                if len(p) >= 2 and p[0].isdigit():
                    pts.append((int(p[0]), float(p[1]) / 1000.0))
    except OSError:
        return None
    pts = [p for p in pts if t0 <= p[0] <= t1]
    if len(pts) < 2:
        return None
    e = sum((b[0] - a[0]) / 1e9 * (a[1] + b[1]) / 2 for a, b in zip(pts, pts[1:]))
    return dict(joules=e, mean_w=e / ((pts[-1][0] - pts[0][0]) / 1e9), samples=len(pts))


# ------------------------------------------------------------------ processes


class Group:
    """A process started in its own process group, killed with it."""

    def __init__(self, argv, env, log=None, cpus=None, stdin=None, stdout=None):
        if cpus:
            argv = [TASKSET, "-c", cpus, *argv]
        self.log = open(log, "w") if log else subprocess.DEVNULL
        self.p = subprocess.Popen(argv, env=env, stdin=stdin or subprocess.DEVNULL,
                                  stdout=stdout or self.log, stderr=self.log, start_new_session=True)
        self.pid = self.p.pid

    def alive(self):
        return self.p.poll() is None

    def stop(self, sig=signal.SIGTERM, wait=5):
        try:
            os.killpg(self.pid, sig)
        except ProcessLookupError:
            return
        try:
            self.p.wait(wait)
        except subprocess.TimeoutExpired:
            try:
                os.killpg(self.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            self.p.wait()


def kill_runtime(rt):
    """Anything left with this runtime directory in its environment."""
    me = os.getpid()
    for p in os.listdir("/proc"):
        if not p.isdigit() or int(p) == me:
            continue
        try:
            with open(f"/proc/{p}/environ", "rb") as f:
                env = f.read().split(b"\0")
        except OSError:
            continue
        if f"XDG_RUNTIME_DIR={rt}".encode() in env:
            try:
                os.kill(int(p), signal.SIGKILL)
            except ProcessLookupError:
                pass


def fonts_conf(path):
    terminal_configs()
    """A fontconfig file naming only Aizome's fonts, with the system's rules."""
    d = os.path.dirname(path)
    os.makedirs(f"{d}/conf.d", exist_ok=True)
    for f in os.listdir("/etc/fonts/conf.d"):
        if f.endswith(".conf") and f != "00-nixos-cache.conf":
            dst = f"{d}/conf.d/{f}"
            if not os.path.exists(dst):
                os.symlink(os.path.realpath(f"/etc/fonts/conf.d/{f}"), dst)
    with open(path, "w") as f:
        f.write('<?xml version="1.0"?>\n<!DOCTYPE fontconfig SYSTEM "urn:fontconfig:fonts.dtd">\n<fontconfig>\n'
                f"  <dir>{os.path.realpath(FONTS)}</dir>\n  <cachedir>{TK}/fc-cache</cachedir>\n"
                f'  <include ignore_missing="yes">{d}/conf.d</include>\n</fontconfig>\n')


def prepare_home(home, blink=False):
    """A subject's home, kept between runs (warm caches), with the settings
    that switch caret blinking off in each toolkit (unless `blink`)."""
    for d in [".config/gtk-4.0", ".config/glib-2.0/settings", ".config/aizome", ".cache", ".local/share", ".local/state"]:
        os.makedirs(f"{home}/{d}", exist_ok=True)
    with open(f"{home}/.config/gtk-4.0/settings.ini", "w") as f:
        f.write("[Settings]\n" + ("" if blink else "gtk-cursor-blink=false\n"))
    with open(f"{home}/.config/glib-2.0/settings/keyfile", "w") as f:
        f.write("[org/gnome/desktop/interface]\n" + ("" if blink else "cursor-blink=false\n"))
    with open(f"{home}/.config/aizome/settings.toml", "w") as f:
        # The kit has no switch for the caret alone: reduced motion stops it
        # blinking (it still redraws the caret on the blink's ticks).
        f.write("" if blink else "[appearance]\nreduced_motion = true\n")
    # KWrite and Dolphin: no session restore, no first-run tips.
    with open(f"{home}/.config/kwriterc", "w") as f:
        f.write("[General]\nShow welcome view for new window=false\n")


def client_env(rt, display, home, extra=None):
    env = {
        "HOME": home, "XDG_RUNTIME_DIR": rt, "WAYLAND_DISPLAY": display,
        "XDG_CONFIG_HOME": f"{home}/.config", "XDG_CACHE_HOME": f"{home}/.cache",
        "XDG_DATA_HOME": f"{home}/.local/share", "XDG_STATE_HOME": f"{home}/.local/state",
        "DBUS_SESSION_BUS_ADDRESS": f"unix:path={rt}/bus",
        "PATH": f"{ROOTS}/sway/bin:/run/current-system/sw/bin",
        "LANG": "C.UTF-8", "LOCALE_ARCHIVE": "/run/current-system/sw/lib/locale/locale-archive",
        "FONTCONFIG_FILE": f"{TK}/fontconfig/fonts.conf",
        "AIZOME_FONT_DIR": os.path.realpath(FONTS),
        "__EGL_VENDOR_LIBRARY_FILENAMES": MESA_EGL, "VK_DRIVER_FILES": RADV,
        "GDK_BACKEND": "wayland", "QT_QPA_PLATFORM": "wayland", "XDG_SESSION_TYPE": "wayland",
        "GSETTINGS_BACKEND": "keyfile", "NO_AT_BRIDGE": "1",
    }
    env.update(extra or {})
    return env


# ------------------------------------------------------------------ sway


SWAY_CONF = """output HEADLESS-1 mode --custom {w}x{h}@{hz}Hz scale {scale} position 0 0 bg #202020 solid_color
xwayland disable
default_border none
default_floating_border none
focus_follows_mouse no
seat * hide_cursor 0
for_window [title=".*"] floating enable, move position 0 0
"""


class Sway:
    """A private headless sway on the Radeon, in its own process group."""

    def __init__(self, rt, w=1920, h=1080, scale=1, hz=60, log=None):
        self.rt, self.w, self.h, self.scale, self.hz = rt, w, h, scale, hz
        self.log = log

    def start(self):
        if os.path.exists(self.rt):
            kill_runtime(self.rt)
            shutil.rmtree(self.rt, ignore_errors=True)
        os.makedirs(self.rt, mode=0o700)
        conf = f"{self.rt}/sway.conf"
        with open(conf, "w") as f:
            f.write(SWAY_CONF.format(w=self.w, h=self.h, hz=self.hz, scale=self.scale))
        env = {
            "XDG_RUNTIME_DIR": self.rt, "HOME": f"{TK}/home/sway", "PATH": f"{ROOTS}/sway/bin:/run/current-system/sw/bin",
            "WLR_BACKENDS": "headless", "WLR_RENDERER": "gles2", "WLR_LIBINPUT_NO_DEVICES": "1",
            "WLR_RENDER_DRM_DEVICE": RENDER_NODE, "__EGL_VENDOR_LIBRARY_FILENAMES": MESA_EGL,
            # No direct scanout: a client buffer covering the output would
            # skip composition for some toolkits and not others.
            "WLR_SCENE_DISABLE_DIRECT_SCANOUT": "1",
            "LANG": "C.UTF-8",
        }
        os.makedirs(env["HOME"], exist_ok=True)
        self.g = Group([f"{ROOTS}/sway/bin/sway", "-c", conf], env, log=self.log, cpus=CPU_COMP)
        for _ in range(200):
            socks = [s for s in os.listdir(self.rt) if re.match(r"wayland-\d+$", s)]
            ipc = [s for s in os.listdir(self.rt) if s.startswith("sway-ipc")]
            if socks and ipc:
                self.display, self.ipc_path = socks[0], f"{self.rt}/{ipc[0]}"
                break
            time.sleep(0.05)
        else:
            raise RuntimeError("sway didn't start")
        time.sleep(0.3)
        self.pid = [p for p in descendants(self.g.pid) if comm(p) == "sway"][0]
        return self

    def ipc(self, kind, payload=""):
        s = socket.socket(socket.AF_UNIX)
        s.connect(self.ipc_path)
        data = payload.encode()
        s.sendall(b"i3-ipc" + struct.pack("<II", len(data), kind) + data)
        hdr = b""
        while len(hdr) < 14:
            hdr += s.recv(14 - len(hdr))
        n, _ = struct.unpack("<II", hdr[6:])
        body = b""
        while len(body) < n:
            body += s.recv(n - len(body))
        s.close()
        return json.loads(body)

    def command(self, cmd):
        return self.ipc(0, cmd)

    def windows(self):
        out = []

        def walk(n):
            if n.get("app_id") is not None or n.get("window_properties"):
                out.append(n)
            for c in n.get("nodes", []) + n.get("floating_nodes", []):
                walk(c)
        walk(self.ipc(4))
        return out

    def find(self, app_id, timeout=20):
        t0 = time.monotonic()
        while time.monotonic() - t0 < timeout:
            for w in self.windows():
                if w.get("app_id") == app_id:
                    return w
            time.sleep(0.05)
        return None

    def stop(self):
        self.g.stop()
        kill_runtime(self.rt)


class Bus:
    def __init__(self, rt):
        self.rt = rt

    def start(self):
        self.g = Group([f"{ROOTS}/dbus/bin/dbus-daemon", "--session", "--nofork", "--nopidfile", f"--address=unix:path={self.rt}/bus"],
                       {"PATH": f"{ROOTS}/dbus/bin:/run/current-system/sw/bin", "XDG_RUNTIME_DIR": self.rt,
                        "HOME": f"{TK}/home/bus"}, cpus=CPU_TOOLS)
        for _ in range(100):
            if os.path.exists(f"{self.rt}/bus"):
                return self
            time.sleep(0.02)
        raise RuntimeError("no bus")

    def stop(self):
        self.g.stop()


class Proxy:
    """wltrace between a client and the compositor."""

    def __init__(self, rt, upstream, name, log):
        self.rt, self.upstream, self.name, self.log = rt, upstream, name, log

    def start(self):
        env = {"XDG_RUNTIME_DIR": self.rt, "WAYLAND_DISPLAY": self.upstream}
        self.g = Group([f"{TOOLS}/wltrace", self.name, self.log], env, cpus=CPU_TOOLS)
        for _ in range(100):
            if os.path.exists(f"{self.rt}/{self.name}"):
                return self
            time.sleep(0.01)
        raise RuntimeError("no proxy")

    def stop(self):
        self.g.stop(signal.SIGTERM)


class Observer:
    """The sway observer: virtual keyboard and pointer, screencopy."""

    def __init__(self, rt, display, log):
        env = {"XDG_RUNTIME_DIR": rt, "WAYLAND_DISPLAY": display}
        self.out = open(log, "w")
        self.g = subprocess.Popen([TASKSET, "-c", CPU_TOOLS, f"{TOOLS}/observer"], env=env, stdin=subprocess.PIPE,
                                  stdout=subprocess.PIPE, stderr=self.out, text=True, bufsize=1, start_new_session=True)
        self.lines = []
        first = self.read()
        assert first["event"] == "ready", first

    def send(self, cmd):
        self.g.stdin.write(cmd + "\n")
        self.g.stdin.flush()

    def read(self, timeout=None):
        import select
        if timeout is not None:
            r, _, _ = select.select([self.g.stdout], [], [], timeout)
            if not r:
                return None
        line = self.g.stdout.readline()
        if not line:
            raise RuntimeError("observer died")
        e = json.loads(line)
        self.out.write(line)
        return e

    def drain(self, seconds):
        """Every line within `seconds`."""
        out, t_end = [], time.monotonic() + seconds
        while True:
            left = t_end - time.monotonic()
            if left <= 0:
                return out
            e = self.read(left)
            if e is None:
                return out
            out.append(e)

    def stop(self):
        try:
            self.send("quit")
            self.g.wait(2)
        except Exception:
            pass
        try:
            os.killpg(self.g.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        self.out.close()


# ------------------------------------------------------------------ the proxy's log


def read_trace(path):
    """The proxy's log as tuples (t_ns, client, event, fields)."""
    out = []
    with open(path) as f:
        for line in f:
            p = line.split()
            if len(p) >= 3 and p[0].isdigit():
                out.append((int(p[0]), int(p[1]), p[2], p[3:]))
    return out
