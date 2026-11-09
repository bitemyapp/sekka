#!/usr/bin/env python3
"""GNOME Shell, KWin with plasmashell, and Aizome, each headless on the
host, for docs/performance/why-desktop.md: memory by mapping, idle work,
one update to an open window, and a window moving.

    headless.py DESKTOP OUTDIR PINS [--settle S] [--idle S] [--updates N]
                [--move-steps N] [--perf] [--wallpaper on|off] [--tag T]

DESKTOP is gnome, plasma or aizome. PINS is a directory of out-links (see
run.sh): the desktops' own packages from the VMs' Nixpkgs, each VM's
system-path (sw) for its plugins, QML modules, schemas and wallpapers, the
probe, perf and dbus.

Isolation: the session runs in bubblewrap with a minimal /dev (no GPU,
input, NVIDIA or udmabuf nodes, so every compositor renders on the CPU:
llvmpipe for GNOME Shell and KWin, pixman for Aizome, as in the VMs), the
live session's runtime directory, X11 sockets and the system bus hidden,
its own PID namespace (it all dies with the sandbox), a private session
bus and an empty private stand-in for the system bus. It is pinned to two
CPUs (taskset), which llvmpipe, GLib and Qt then size their thread pools
by, as in the 2-vCPU VMs. Started with setsid; the whole group is killed
at the end.

Phases (each process of the session measured from /proc from outside):
  settle      SETTLE seconds after start
  memory      every process's smaps, classified by mapping (memory.json)
  idle        IDLE seconds untouched: per-thread wakeups and CPU
  updates     the probe maps an 800x600 window and flips it N times,
              120-250 ms apart: CPU of the compositor and shell per update
  move        the probe's window moved MOVE_STEPS times, 16 ms apart, by
              the desktop's own scripting (GNOME: Shell.Eval; KWin: a
              KWin script; Aizome: aizomectl move): CPU per step
With --perf, `perf record -g` runs on the compositor and shell during idle,
updates and move (attribution only; the CPU figures from a --perf run are
not reported as timings).
"""
import json
import os
import random
import re
import shutil
import signal
import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from aizome_session import private_bus_shell  # noqa: E402

CPUS = os.environ.get("WHY_CPUS", "2,3")
BWRAP = shutil.which("bwrap") or "bwrap"
TASKSET = shutil.which("taskset") or "taskset"
NSENTER = shutil.which("nsenter") or "nsenter"


def inside(pid):
    """Run in the sandbox's user, PID and network namespaces (entering a
    user namespace one owns needs no privilege). Clients have to: smithay's
    wayland-backend panics when it can't read a client's credentials, as
    for a process outside the compositor's PID namespace."""
    return [NSENTER, "-t", str(pid), "-U", "-p", "-n", "--preserve-credentials", "--"]
HZ = os.sysconf("SC_CLK_TCK")


def die(msg):
    print(msg, file=sys.stderr)
    sys.exit(1)


# ---------------------------------------------------------------- /proc


def descendants(pid):
    out, todo = [], [pid]
    while todo:
        p = todo.pop()
        out.append(p)
        try:
            tasks = os.listdir(f"/proc/{p}/task")
        except OSError:
            continue
        for t in tasks:
            try:
                with open(f"/proc/{p}/task/{t}/children") as f:
                    todo += [int(x) for x in f.read().split()]
            except OSError:
                pass
    return out


def argv0(pid):
    try:
        with open(f"/proc/{pid}/cmdline", "rb") as f:
            a = f.read().split(b"\0")[0].decode(errors="replace")
    except OSError:
        return "?"
    n = os.path.basename(a).lstrip(".")
    return re.sub(r"-wrapped$", "", n)


def threads(pid):
    out = {}
    try:
        tids = os.listdir(f"/proc/{pid}/task")
    except OSError:
        return out
    for t in tids:
        try:
            with open(f"/proc/{pid}/task/{t}/stat") as f:
                s = f.read()
            comm = s[s.index("(") + 1:s.rindex(")")]
            fields = s[s.rindex(")") + 2:].split()
            with open(f"/proc/{pid}/task/{t}/status") as f:
                st = f.read()
            vol = int(re.search(r"^voluntary_ctxt_switches:\s+(\d+)", st, re.M).group(1))
            inv = int(re.search(r"^nonvoluntary_ctxt_switches:\s+(\d+)", st, re.M).group(1))
            with open(f"/proc/{pid}/task/{t}/schedstat") as f:
                run_ns = int(f.read().split()[0])
            out[t] = dict(comm=comm, vol=vol, inv=inv, run_ns=run_ns,
                          ticks=int(fields[11]) + int(fields[12]))
        except (OSError, ValueError, AttributeError):
            pass
    return out


def snapshot(pids):
    return {p: dict(name=argv0(p), threads=threads(p)) for p in pids}


def delta(a, b, dt):
    """Per process and per thread: wakeups/s and CPU ms/s between snapshots."""
    out = {}
    for p, pa in a.items():
        pb = b.get(p)
        if not pb:
            continue
        rows, tot_w, tot_c = [], 0.0, 0.0
        for t, ta in pa["threads"].items():
            tb = pb["threads"].get(t)
            if not tb:
                continue
            w = (tb["vol"] - ta["vol"]) / dt
            c = (tb["run_ns"] - ta["run_ns"]) / 1e6 / dt
            tot_w += w
            tot_c += c
            if w > 0.05 or c > 0.01:
                rows.append(dict(thread=tb["comm"], wakeups_per_s=round(w, 2), cpu_ms_per_s=round(c, 3)))
        rows.sort(key=lambda r: -r["wakeups_per_s"])
        out[f'{pa["name"]}[{p}]'] = dict(name=pa["name"], wakeups_per_s=round(tot_w, 2),
                                         cpu_ms_per_s=round(tot_c, 3), threads=rows)
    return out


# ---------------------------------------------------------------- memory

LIB = re.compile(r"\.so(\.[0-9.]+)?$")
FONT = re.compile(r"\.(ttf|otf|ttc|pcf|pcf\.gz|woff2?)$", re.I)
ICON = re.compile(r"(icon-theme\.cache|/icons/.*\.(png|svg|svgz)$|\.icns$)")
CACHES = re.compile(r"(fontconfig/.*cache|\.cache/|ksycoca|mesa_shader|qmlcache|\.qmlc$|\.jsc$|gschemas\.compiled|"
                    r"mime\.cache|/mime/|locale-archive|\.gresource$|\.rcc$|icon-cache\.kcache|plasma-svgelements|"
                    r"\.kcache)")


def parse_smaps(pid):
    maps, cur = [], None
    try:
        with open(f"/proc/{pid}/smaps") as f:
            for line in f:
                parts = line.split()
                if not parts:
                    continue
                if re.match(r"^[0-9a-f]+-[0-9a-f]+$", parts[0]):
                    lo, hi = (int(x, 16) for x in parts[0].split("-"))
                    cur = dict(lo=lo, hi=hi, perms=parts[1], path=" ".join(parts[5:]) if len(parts) > 5 else "",
                               inode=parts[4])
                    maps.append(cur)
                elif cur is not None and len(parts) >= 3 and parts[2] == "kB":
                    cur[parts[0].rstrip(":")] = int(parts[1])
    except OSError:
        pass
    return maps


def classify(maps, gc_chunk_heuristic):
    """Each mapping's PSS into a category. File mappings' copied-on-write
    pages (relocated data) are counted as their library's anonymous part."""
    cats = {}

    def add(cat, pss, anon=0, rss=0):
        c = cats.setdefault(cat, dict(pss_kib=0, anon_kib=0, rss_kib=0, mappings=0))
        c["pss_kib"] += pss
        c["anon_kib"] += anon
        c["rss_kib"] += rss
        c["mappings"] += 1

    n = len(maps)
    for i, m in enumerate(maps):
        pss, anon, rss = m.get("Pss", 0), m.get("Anonymous", 0), m.get("Rss", 0)
        if rss == 0 and pss == 0:
            continue
        path, perms = m["path"], m["perms"]
        size = m["hi"] - m["lo"]
        if path == "[heap]":
            add("malloc: main heap (brk)", pss, anon, rss)
        elif path.startswith("[stack"):
            add("thread stacks", pss, anon, rss)
        elif path in ("[vdso]", "[vvar]", "[vsyscall]", "[vvar_vclock]", "[uprobes]"):
            add("kernel-provided", pss, anon, rss)
        elif path.startswith("/memfd:JSGCHeap"):
            add("JS GC heap (Qt's QML engine, named memfd)", pss, anon, rss)
        elif path.startswith("/memfd:mesa-shared"):
            add("llvmpipe display buffers (memfd shared with the compositor)", pss, anon, rss)
        elif path.startswith("/memfd:") or path.startswith("/dev/shm") or "SYSV" in path or \
                (path and m.get("Shared_Dirty", 0) + m.get("Shared_Clean", 0) > 0 and "(deleted)" in path):
            add("shared memory (window buffers, wl_shm pools)", pss, anon, rss)
        elif path.startswith("/dev/dri") or path.startswith("/dev/"):
            add("device mappings", pss, anon, rss)
        elif path and not path.startswith("["):
            base = path.split(" (deleted)")[0]
            if LIB.search(base):
                if "x" in perms:
                    add("libraries: code", pss, anon, rss)
                elif anon:
                    # relocated data: private dirty pages, counted as anonymous
                    add("libraries: relocated data (private)", anon, anon, rss)
                    if pss - anon > 0:
                        add("libraries: read-only data", pss - anon, 0, 0)
                else:
                    add("libraries: read-only data", pss, anon, rss)
            elif FONT.search(base):
                add("fonts (files mapped)", pss, anon, rss)
            elif ICON.search(base):
                add("icons and icon caches", pss, anon, rss)
            elif CACHES.search(base):
                add("caches, schemas, locale, resources", pss, anon, rss)
            elif "/bin/" in base or "/libexec/" in base:
                add("executable", pss, anon, rss)
            else:
                add("other files", pss, anon, rss)
        else:
            # anonymous
            if "x" in perms:
                add("anonymous executable (JIT code)", pss, anon, rss)
                continue
            if perms.startswith("---"):
                add("reserved, no access", pss, anon, rss)
                continue
            nxt = maps[i + 1] if i + 1 < n else None
            prv = maps[i - 1] if i > 0 else None
            arena = (m["lo"] % (64 << 20) == 0 and nxt is not None and nxt["perms"].startswith("---")
                     and nxt["path"] == "" and nxt["lo"] == m["hi"]
                     and (nxt["hi"] - m["lo"]) == (64 << 20))
            stack = (prv is not None and prv["perms"].startswith("---") and prv["path"] == ""
                     and prv["hi"] == m["lo"] and (prv["hi"] - prv["lo"]) <= (64 << 10)
                     and (size in (8 << 20, (8 << 20) + 4096) or 2 << 20 >= size >= 64 << 10))
            if arena or (m["lo"] % (64 << 20) == 0 and size == (64 << 20)):
                add("malloc: thread arenas", pss, anon, rss)
            elif stack:
                add("thread stacks", pss, anon, rss)
            elif gc_chunk_heuristic and m["lo"] % (1 << 20) == 0 and size % (1 << 20) == 0:
                add("JS GC heap (1 MiB-aligned chunks)", pss, anon, rss)
            else:
                add("other anonymous mmap (large malloc blocks, engines' own heaps, driver buffers)", pss, anon, rss)
    return cats


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


def top_files(maps, k=25):
    agg = {}
    for m in maps:
        p = m["path"]
        if not p or p.startswith("["):
            continue
        p = re.sub(r"^/nix/store/[a-z0-9]{32}-", "/nix/store/…-", p)
        a = agg.setdefault(p, [0, 0])
        a[0] += m.get("Pss", 0)
        a[1] += m.get("Anonymous", 0)
    return [dict(path=p, pss_kib=v[0], anon_kib=v[1]) for p, v in sorted(agg.items(), key=lambda kv: -kv[1][0])[:k]]


# ---------------------------------------------------------------- sessions


def session_env(desk, root, pins, wallpaper):
    """wallpaper: True (Sekka's), False (none), or a path to a picture."""
    sw = f"{pins}/{desk}-sw"
    home, rt = f"{root}/home", f"{root}/rt"
    env = {
        "HOME": home, "USER": "demo", "LOGNAME": "demo",
        "XDG_RUNTIME_DIR": rt,
        "XDG_CONFIG_HOME": f"{home}/.config", "XDG_CACHE_HOME": f"{home}/.cache",
        "XDG_DATA_HOME": f"{home}/.local/share", "XDG_STATE_HOME": f"{home}/.local/state",
        "PATH": f"{sw}/bin:{pins}/dbus/bin:/run/current-system/sw/bin",
        "LANG": "en_US.UTF-8", "TZ": "UTC",
        "LOCALE_ARCHIVE": "/run/current-system/sw/lib/locale/locale-archive",
        "XDG_DATA_DIRS": f"{sw}/share", "XDG_CONFIG_DIRS": f"{root}/xdg:{sw}/etc/xdg",
        "XDG_SESSION_TYPE": "wayland",
        "__EGL_VENDOR_LIBRARY_FILENAMES": "/run/opengl-driver/share/glvnd/egl_vendor.d/50_mesa.json",
        "LIBGL_ALWAYS_SOFTWARE": "1", "GALLIUM_DRIVER": "llvmpipe", "LP_NUM_THREADS": "2",
        "DBUS_SESSION_BUS_ADDRESS": f"unix:path={rt}/bus",
        "DBUS_SYSTEM_BUS_ADDRESS": f"unix:path={rt}/system_bus",
        "QT_FORCE_STDERR_LOGGING": "1",
    }
    walls = f"{sw}/share/backgrounds/tatami"
    if desk == "gnome":
        env.update({"XDG_CURRENT_DESKTOP": "GNOME", "XDG_SESSION_DESKTOP": "gnome", "GSETTINGS_BACKEND": "keyfile"})
        kf = f"{home}/.config/glib-2.0/settings/keyfile"
        os.makedirs(os.path.dirname(kf), exist_ok=True)
        with open(kf, "w") as f:
            if wallpaper:
                w = wallpaper if isinstance(wallpaper, str) else os.path.realpath(f"{walls}/foggy-forest.jpg")
                f.write("[org/gnome/desktop/background]\n"
                        f"picture-uri='file://{w}'\npicture-uri-dark='file://{w}'\npicture-options='zoom'\n\n"
                        "[org/gnome/desktop/screensaver]\n"
                        f"picture-uri='file://{w}'\npicture-options='zoom'\n\n")
            else:
                f.write("[org/gnome/desktop/background]\n"
                        "picture-uri=''\npicture-uri-dark=''\npicture-options='none'\n"
                        "primary-color='#204a87'\n\n")
            f.write("[org/gnome/shell]\nwelcome-dialog-last-shown-version='999'\n")
        cmd = [f"{pins}/gnome-shell/bin/gnome-shell", "--wayland", "--headless", "--virtual-monitor",
               "1920x1080@75", "--wayland-display", "wayland-bench",
               "--automation-script", f"{root}/automation.js"]
        with open(f"{root}/automation.js", "w") as f:
            f.write(GNOME_AUTOMATION)
        os.makedirs(f"{home}/.config", exist_ok=True)
        open(f"{home}/.config/gnome-initial-setup-done", "w").write("yes")
    elif desk == "plasma":
        env.update({
            "XDG_CURRENT_DESKTOP": "KDE", "XDG_SESSION_DESKTOP": "KDE", "KDE_FULL_SESSION": "true",
            "KDE_SESSION_VERSION": "6",
            "QT_PLUGIN_PATH": f"{sw}/lib/qt-6/plugins", "QML2_IMPORT_PATH": f"{sw}/lib/qt-6/qml",
            "NIXPKGS_QT6_QML_IMPORT_PATH": f"{sw}/lib/qt-6/qml",
            "KWIN_COMPOSE": "O2",
            "QT_LOGGING_RULES": "qt.qml.gc.statistics=true;qt.qml.gc.allocatorStats=true",
        })
        os.makedirs(f"{root}/xdg", exist_ok=True)
        with open(f"{root}/xdg/kdeglobals", "w") as f:
            f.write("[KDE]\nLookAndFeelPackage=org.nixos.breeze-ocean.desktop\n" if wallpaper else "")
        cmd = [f"{pins}/kwin/bin/kwin_wayland", "--virtual", "--width", "1920", "--height", "1080",
               "--socket", "wayland-bench", "--no-lockscreen", "--xwayland",
               # as startplasma does: the session's environment for D-Bus
               # activation (portals, kactivitymanagerd), then the shell
               "env QT_QPA_PLATFORM=wayland sh -c 'dbus-update-activation-environment --all; exec plasmashell'"]
    elif desk == "aizome":
        w = wallpaper if isinstance(wallpaper, str) else os.path.realpath(f"{walls}/tokyo-at-dusk.jpg")
        # The pinned package's shell and helpers, not the system path's
        # (the compositor finds the shell on PATH, session.rs shell_binary).
        env["PATH"] = f"{pins}/aizome/bin:" + env["PATH"]
        env["AIZOME_SHELL_BIN"] = private_bus_shell(
            root, env["DBUS_SESSION_BUS_ADDRESS"], f"{pins}/aizome/bin/aizome-shell")
        env.update({"XDG_CURRENT_DESKTOP": "Aizome", "XDG_SESSION_DESKTOP": "aizome",
                    "AIZOME_WALLPAPER_DAY": w if wallpaper else "",
                    "AIZOME_WALLPAPER_NIGHT": w if wallpaper else "",
                    "DCONF_PROFILE": "aizome", "QT_QPA_PLATFORM": "wayland;xcb"})
        cmd = [f"{pins}/aizome/bin/aizome", "--backend", "headless", "--headless-output", "1920x1080@1",
               "--renderer", "pixman", "--socket", "wayland-bench"]
    else:
        die(f"unknown desktop {desk}")
    return env, cmd


GNOME_AUTOMATION = r"""
// Loaded by gnome-shell --automation-script: leave the overview, allow
// Shell.Eval for the harness, and keep the shell running until killed.
import * as Main from 'resource:///org/gnome/shell/ui/main.js';
export const METRICS = {};
export async function run() {
    global.context.unsafe_mode = true;
    Main.overview.hide();
    await new Promise(() => {});
}
"""

SYSTEM_BUS_CONF = """<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <type>system</type>
  <listen>unix:path={path}</listen>
  <auth>EXTERNAL</auth>
  <policy context="default">
    <allow send_destination="*" eavesdrop="true"/>
    <allow eavesdrop="true"/>
    <allow own="*"/>
  </policy>
</busconfig>
"""

INNER = """#!/bin/sh
# Inside the sandbox: a private stand-in for the system bus, a session bus,
# then the compositor in the foreground.
set -e
dbus-daemon --nofork --nopidfile --config-file="$WHY_ROOT/system-bus.conf" &
dbus-daemon --session --nofork --nopidfile --address="unix:path=$XDG_RUNTIME_DIR/bus" &
for i in $(seq 50); do
  [ -S "$XDG_RUNTIME_DIR/bus" ] && [ -S "$XDG_RUNTIME_DIR/system_bus" ] && break
  sleep 0.05
done
exec "$@" > "$WHY_ROOT/session.log" 2>&1
"""


def start(desk, root, pins, wallpaper, perf_out=None):
    if os.path.exists(root):
        shutil.rmtree(root)
    os.makedirs(f"{root}/rt", mode=0o700)
    os.makedirs(f"{root}/home")
    env, cmd = session_env(desk, root, pins, wallpaper)
    env["WHY_ROOT"] = root
    with open(f"{root}/system-bus.conf", "w") as f:
        f.write(SYSTEM_BUS_CONF.format(path=f"{root}/rt/system_bus"))
    with open(f"{root}/inner.sh", "w") as f:
        f.write(INNER)
    # KWin's software renderer (llvmpipe) allocates its buffers through
    # udmabuf (core/gpumanager.cpp); it is the one device node it gets.
    devs = ["--dev-bind", "/dev/udmabuf", "/dev/udmabuf"] if desk == "plasma" else []
    # A private /tmp (only the harness's own directory bound back in) and a
    # private network namespace: X servers pick their display by lock files
    # in /tmp and abstract sockets, and a compositor in its own PID
    # namespace would take the host's lock files for stale ones (Mutter
    # unlinks a lock whose PID it can't see, meta-xwayland.c).
    sandbox = [TASKSET, "-c", CPUS, BWRAP, "--dev-bind", "/", "/", "--dev", "/dev", *devs, "--tmpfs", "/dev/shm",
               "--proc", "/proc",
               "--tmpfs", "/tmp", "--bind", "/tmp/claude-1000", "/tmp/claude-1000",
               "--perms", "1777", "--tmpfs", "/tmp/.X11-unix",
               "--tmpfs", "/run/user/1000", "--tmpfs", "/run/dbus", "--tmpfs", "/run/systemd/journal",
               "--unshare-pid", "--unshare-net", "--die-with-parent", "--chdir", f"{root}/home",
               "sh", f"{root}/inner.sh", *cmd]
    if perf_out:
        # perf has to be the session's ancestor (Yama ptrace_scope 1 lets it
        # attach to nothing else, and not into the sandbox's namespaces);
        # task-clock is the event an unprivileged user may sample. Samples
        # carry CLOCK_MONOTONIC, so each phase is reported by its time window
        # (record["perf_windows"], perf-summary.sh).
        sandbox = [f"{pins}/perf/bin/perf", "record", "-q", "-e", "task-clock", "-F", "1999",
                   "-k", "CLOCK_MONOTONIC", "-o", perf_out, "--", *sandbox]
    proc = subprocess.Popen(sandbox, env=env, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                            stderr=open(f"{root}/sandbox.log", "w"), start_new_session=True)
    return proc, env


def stop(proc, extra=()):
    for p in [proc, *extra]:
        if p is None:
            continue
        try:
            os.killpg(p.pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
    deadline = time.monotonic() + 8
    for p in [proc, *extra]:
        if p is None:
            continue
        try:
            p.wait(max(0.1, deadline - time.monotonic()))
        except subprocess.TimeoutExpired:
            try:
                os.killpg(p.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            p.wait()


ROLES = {
    "gnome": {"gnome-shell": "compositor+shell"},
    "plasma": {"kwin_wayland": "compositor", "plasmashell": "shell"},
    "aizome": {"aizome": "compositor", "aizome-shell": "shell"},
}


def session_pids(proc):
    return [p for p in descendants(proc.pid) if p != proc.pid]


def main_pids(desk, pids):
    want = ROLES[desk]
    return {p: want[argv0(p)] for p in pids if argv0(p) in want}


def gnome_eval(env, js):
    r = subprocess.run(["gdbus", "call", "--session", "--dest", "org.gnome.Shell", "--object-path", "/org/gnome/Shell",
                        "--method", "org.gnome.Shell.Eval", js], env=env, capture_output=True, text=True, timeout=20)
    return (r.stdout + r.stderr).strip()


def kwin_script(env, root, js):
    path = f"{root}/move.js"
    with open(path, "w") as f:
        f.write(js)
    r = subprocess.run(["gdbus", "call", "--session", "--dest", "org.kde.KWin", "--object-path", "/Scripting",
                        "--method", "org.kde.kwin.Scripting.loadScript", path, "why-move"],
                       env=env, capture_output=True, text=True, timeout=20)
    m = re.search(r"\((\d+),\)", r.stdout)
    if not m:
        return f"load failed: {r.stdout} {r.stderr}"
    sid = m.group(1)
    r2 = subprocess.run(["gdbus", "call", "--session", "--dest", "org.kde.KWin", "--object-path",
                         f"/Scripting/Script{sid}", "--method", "org.kde.kwin.Script.run"],
                        env=env, capture_output=True, text=True, timeout=20)
    return (r2.stdout + r2.stderr).strip()


PERF_MARKS = {}


def perf_phase(proc, out, phase):
    """Mark the end of a phase (and the start of the next) for perf's time windows."""
    if proc is None:
        return
    PERF_MARKS[phase] = time.clock_gettime(time.CLOCK_MONOTONIC)


def perf_start(pins, pids, out):
    if not pids:
        return None
    return subprocess.Popen([f"{pins}/perf/bin/perf", "record", "-q", "-F", "1999", "-g", "-o", out,
                             "-p", ",".join(str(p) for p in pids)],
                            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True)


def perf_stop(p):
    if p is None:
        return
    os.killpg(p.pid, signal.SIGINT)
    try:
        p.wait(30)
    except subprocess.TimeoutExpired:
        os.killpg(p.pid, signal.SIGKILL)
        p.wait()


def main():
    args = sys.argv[1:]
    if len(args) < 3:
        die(__doc__)
    desk, outdir, pins = args[0], args[1], os.path.abspath(args[2])
    settle, idle, updates, move_steps, use_perf, wallpaper, tag = 40.0, 30.0, 120, 300, False, True, "r1"
    rest = args[3:]
    while rest:
        a = rest.pop(0)
        if a == "--settle":
            settle = float(rest.pop(0))
        elif a == "--idle":
            idle = float(rest.pop(0))
        elif a == "--updates":
            updates = int(rest.pop(0))
        elif a == "--move-steps":
            move_steps = int(rest.pop(0))
        elif a == "--perf":
            use_perf = True
        elif a == "--wallpaper":
            v = rest.pop(0)
            wallpaper = True if v == "on" else False if v == "off" else os.path.abspath(v)
        elif a == "--tag":
            tag = rest.pop(0)
        else:
            die(f"unknown argument {a}")
    name = f"{desk}-{tag}" + ("-perf" if use_perf else "") + \
        ("" if wallpaper is True else "-nowall" if wallpaper is False else "-ownwall")
    out = os.path.join(outdir, name)
    os.makedirs(out, exist_ok=True)
    root = f"/tmp/claude-1000/w/{desk}"
    record = dict(desktop=desk, tag=tag, perf=use_perf, wallpaper=wallpaper, cpus=CPUS, settle_s=settle,
                  date=time.strftime("%Y-%m-%dT%H:%M:%S%z"), kernel=os.uname().release,
                  loadavg_start=open("/proc/loadavg").read().split()[:3],
                  packages={k: os.path.realpath(f"{pins}/{k}") for k in
                            ("gnome-shell", "mutter", "kwin", "plasma-workspace", "aizome", f"{desk}-sw", "probe")})
    proc, env = start(desk, root, pins, wallpaper, perf_out=f"{out}/perf.data" if use_perf else None)
    pp = proc if use_perf else None
    probe = mover = None
    try:
        time.sleep(settle)
        pids = session_pids(proc)
        mains = main_pids(desk, pids)
        record["processes"] = {str(p): argv0(p) for p in pids}
        record["main"] = {str(p): r for p, r in mains.items()}
        if not mains:
            raise RuntimeError(f"no compositor found; see {root}/session.log")
        # ---- memory
        mem = {}
        for p in pids:
            n = argv0(p)
            maps = parse_smaps(p)
            r = rollup(p)
            if not r:
                continue
            entry = dict(name=n, rollup={k: r.get(k, 0) for k in ("Rss", "Pss", "Pss_Anon", "Pss_File",
                                                                   "Pss_Shmem", "Anonymous", "Private_Dirty")})
            if p in mains or r.get("Pss", 0) >= 5 * 1024:
                entry["categories"] = classify(maps, gc_chunk_heuristic=(p in mains and desk == "gnome"))
                entry["top_files"] = top_files(maps, 25 if p in mains else 8)
                entry["threads"] = len(os.listdir(f"/proc/{p}/task"))
            mem[f"{n}[{p}]"] = entry
        record["memory"] = mem
        if desk == "gnome":
            record["gnome_eval_gc"] = gnome_eval(env, "imports.system.gc(); 'gc done'")
            time.sleep(2)
            for p in mains:
                record["memory"][f"{argv0(p)}[{p}]"]["rollup_after_gc"] = rollup(p)
                record["memory"][f"{argv0(p)}[{p}]"]["categories_after_gc"] = classify(parse_smaps(p), True)
        # ---- idle
        watch = list(mains)
        allp = session_pids(proc)
        perf_phase(pp, out, "settle")
        perf = None
        a = snapshot(allp)
        t0 = time.monotonic()
        time.sleep(idle)
        b = snapshot(session_pids(proc))
        perf_phase(pp, out, "idle")
        record["idle"] = dict(seconds=round(time.monotonic() - t0, 2), processes=delta(a, b, time.monotonic() - t0))
        # ---- updates
        penv = dict(env)
        penv["WAYLAND_DISPLAY"] = "wayland-bench"
        if os.environ.get("WHY_PROBE_DEBUG"):
            penv["WAYLAND_DEBUG"] = "1"
        status = f"{root}/probe.status"
        hold = move_steps * 0.016 + 12
        anchor = next(iter(mains))
        probe = subprocess.Popen([*inside(anchor), TASKSET, "-c", CPUS, f"{pins}/probe/bin/commit-probe", str(updates),
                                  f"{out}/updates.tsv", status, str(hold)],
                                 env=penv, stdout=subprocess.DEVNULL, stderr=open(f"{out}/probe.log", "w"),
                                 start_new_session=True)
        deadline = time.monotonic() + 40
        while time.monotonic() < deadline:
            s = open(status).read().strip() if os.path.exists(status) else ""
            if s.startswith("ready") or s.startswith("not-ready") or s.startswith("no-"):
                break
            time.sleep(0.05)
        record["probe_status"] = s
        if not s.startswith("ready"):
            raise RuntimeError(f"probe: {s}")
        perf_phase(pp, out, "between")
        perf = None
        a = snapshot(watch)
        t0 = time.monotonic()
        while time.monotonic() < t0 + updates * 0.25 + 10:
            s = open(status).read().strip()
            if s.startswith("updates-done"):
                break
            time.sleep(0.05)
        dt = time.monotonic() - t0
        b = snapshot(watch)
        perf_phase(pp, out, "updates")
        record["updates"] = dict(count=updates, seconds=round(dt, 2), processes=delta(a, b, dt))
        # ---- move
        time.sleep(1)
        perf_phase(pp, out, "between2")
        perf = None
        a = snapshot(watch)
        t0 = time.monotonic()
        if desk == "gnome":
            js = ("(() => { const GLib = imports.gi.GLib; const w = global.get_window_actors().map(a => a.meta_window)"
                  ".find(w => w.get_title() === 'commit-probe'); if (!w) return 'no window'; let i = 0; "
                  f"GLib.timeout_add(GLib.PRIORITY_DEFAULT, 16, () => {{ w.move_frame(true, 100 + (i % 200) * 4, 100 + (i % 100) * 2); "
                  f"return ++i < {move_steps} ? GLib.SOURCE_CONTINUE : GLib.SOURCE_REMOVE; }}); return 'moving'; }})()")
            record["move_start"] = gnome_eval(env, js)
            time.sleep(move_steps * 0.016 + 1.5)
        elif desk == "plasma":
            js = ("const w = workspace.windowList().find(c => c.caption === 'commit-probe');\n"
                  "let i = 0; const t = new QTimer(); t.interval = 16;\n"
                  "t.timeout.connect(() => { const g = w.frameGeometry; "
                  "w.frameGeometry = {x: 100 + (i % 200) * 4, y: 100 + (i % 100) * 2, width: g.width, height: g.height};"
                  f" if (++i >= {move_steps}) t.stop(); }});\nt.start();\n")
            record["move_start"] = kwin_script(env, root, js)
            time.sleep(move_steps * 0.016 + 1.5)
        else:
            ctl = f"{pins}/aizome/bin/aizomectl"
            cenv = dict(env)
            cenv["AIZOME_SOCKET"] = f"{root}/rt/aizome.wayland-bench.sock"
            wins = subprocess.run([*inside(anchor), ctl, "--json", "windows"], env=cenv, capture_output=True, text=True,
                                  timeout=10)
            wid = None

            def find(o):
                if isinstance(o, dict):
                    if o.get("title") == "commit-probe" and "id" in o:
                        i = o["id"]
                        while isinstance(i, (dict, list)):
                            i = next(iter(i.values())) if isinstance(i, dict) else i[0]
                        return i
                    o = list(o.values())
                if isinstance(o, list):
                    for x in o:
                        r = find(x)
                        if r is not None:
                            return r
                return None
            try:
                wid = find(json.loads(wins.stdout))
            except json.JSONDecodeError:
                pass
            record["aizome_windows"] = wins.stdout[:2000]
            record["move_start"] = f"window {wid}"
            for i in range(move_steps):
                nxt = t0 + (i + 1) * 0.016
                subprocess.run([*inside(anchor), ctl, "move", str(wid), str(100 + (i % 200) * 4), str(100 + (i % 100) * 2)],
                               env=cenv, capture_output=True, timeout=10)
                time.sleep(max(0, nxt - time.monotonic()))
            time.sleep(1.5)
        dt = time.monotonic() - t0
        b = snapshot(watch)
        perf_phase(pp, out, "move")
        record["move"] = dict(steps=move_steps, seconds=round(dt, 2), processes=delta(a, b, dt))
        record["loadavg_end"] = open("/proc/loadavg").read().split()[:3]
        if use_perf:
            m = PERF_MARKS
            record["perf_windows"] = {ph: [round(m[a], 6), round(m[b], 6)] for ph, a, b in
                                      (("idle", "settle", "idle"), ("updates", "between", "updates"),
                                       ("move", "between2", "move")) if a in m and b in m}
    except Exception as e:  # keep what was measured
        record["error"] = repr(e)
    finally:
        stop(proc, [probe])
        for log in ("session.log", "sandbox.log"):
            try:
                shutil.copy(f"{root}/{log}", f"{out}/{log}")
            except OSError:
                pass
        left = [p for p in descendants(proc.pid) if p != proc.pid] if proc.poll() is None else []
        record["leftover_processes"] = left
        with open(f"{out}/record.json", "w") as f:
            json.dump(record, f, indent=1)
        shutil.rmtree(root, ignore_errors=True)
    print(json.dumps({k: record.get(k) for k in ("desktop", "tag", "probe_status", "move_start", "error",
                                                  "leftover_processes")}))


if __name__ == "__main__":
    main()
