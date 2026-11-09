#!/usr/bin/env python3
"""One trial of the native comparison (docs/performance/why-toolkits.md):
each toolkit under its own desktop's compositor, headless on the Radeon.

    native_trial.py DESKTOP SUBJECT SCENARIO OUTDIR [--keys N] [--rate R]
                    [--seconds S] [--args ARGS] [--seed N]

DESKTOP is gnome (GNOME Shell 50 / Mutter, --headless --virtual-monitor),
kde (KWin 6.8, --virtual, OpenGL compositing; Qt with Plasma's platform
theme) or aizome (Aizome's compositor, --backend headless, GLES, with its
shell). Scenarios as in sway_trial.py: keys (typing, traced by the proxy),
scroll and idle. Input goes through each compositor's own path: Clutter's
virtual keyboard from GNOME Shell's JavaScript (Shell.Eval), KWin's
fake-input protocol, Aizome's IPC (synthesize). Nothing copies the screen:
times end at the program's frame callback ("done") and, where the program
asks for it, wp_presentation's "presented".

Isolation, as tests/performance/why-desktop/headless.py's: the compositor
runs in bubblewrap with a private /tmp (and /tmp/.X11-unix), its own PID and
network namespaces and a minimal /dev holding only the Radeon's render node
(and /dev/udmabuf), a private session bus and an empty stand-in for the
system bus; X11 is off (Mutter --no-x11, KWin without --xwayland, Aizome
--no-xwayland). Clients are started inside its namespaces (nsenter), as a
compositor that reads its clients' credentials needs."""
import argparse
import json
import os
import random
import shutil
import signal
import socket
import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import tk  # noqa: E402

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from aizome_session import private_bus_shell  # noqa: E402

KEY_A, KEY_BACKSPACE = 30, 14
R = tk.ROOTS
BWRAP = f"{R}/bwrap/bin/bwrap"
NSENTER = f"{R}/util-linux-bin/bin/nsenter"
GDBUS = f"{R}/glib-bin-bin/bin/gdbus"
SOCK = "wayland-tk"

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
  <policy context="default"><allow send_destination="*" eavesdrop="true"/><allow eavesdrop="true"/><allow own="*"/></policy>
</busconfig>
"""

INNER = """#!/bin/sh
set -e
dbus-daemon --nofork --nopidfile --config-file="$TK_ROOT/system-bus.conf" &
dbus-daemon --session --nofork --nopidfile --address="unix:path=$XDG_RUNTIME_DIR/bus" &
for i in $(seq 50); do
  [ -S "$XDG_RUNTIME_DIR/bus" ] && [ -S "$XDG_RUNTIME_DIR/system_bus" ] && break
  sleep 0.05
done
exec "$@" > "$TK_ROOT/compositor.log" 2>&1
"""

COMPOSITOR = {"gnome": "gnome-shell", "kde": "kwin_wayland", "aizome": "aizome"}


def session(desk, root):
    """The compositor's environment and command."""
    home, rt = f"{root}/home", f"{root}/rt"
    env = {
        "HOME": home, "USER": "demo", "LOGNAME": "demo", "XDG_RUNTIME_DIR": rt,
        "XDG_CONFIG_HOME": f"{home}/.config", "XDG_CACHE_HOME": f"{home}/.cache",
        "XDG_DATA_HOME": f"{home}/.local/share", "XDG_STATE_HOME": f"{home}/.local/state",
        "LANG": "C.UTF-8", "TZ": "UTC", "LOCALE_ARCHIVE": "/run/current-system/sw/lib/locale/locale-archive",
        "XDG_SESSION_TYPE": "wayland", "__EGL_VENDOR_LIBRARY_FILENAMES": tk.MESA_EGL, "VK_DRIVER_FILES": tk.RADV,
        "DBUS_SESSION_BUS_ADDRESS": f"unix:path={rt}/bus", "DBUS_SYSTEM_BUS_ADDRESS": f"unix:path={rt}/system_bus",
        "FONTCONFIG_FILE": f"{tk.TK}/fontconfig/fonts.conf", "TK_ROOT": root,
    }
    if desk == "gnome":
        sw = f"{R}/gnome-sw"
        env.update({"PATH": f"{sw}/bin:{R}/dbus/bin:/run/current-system/sw/bin", "XDG_DATA_DIRS": f"{sw}/share",
                    "XDG_CONFIG_DIRS": f"{sw}/etc/xdg", "XDG_CURRENT_DESKTOP": "GNOME", "XDG_SESSION_DESKTOP": "gnome",
                    "GSETTINGS_BACKEND": "keyfile"})
        with open(f"{root}/automation.js", "w") as f:
            f.write(GNOME_AUTOMATION)
        cmd = [f"{R}/gnome-shell/bin/gnome-shell", "--wayland", "--headless", "--no-x11", "--virtual-monitor",
               "1920x1080@60", "--wayland-display", SOCK, "--automation-script", f"{root}/automation.js"]
    elif desk == "kde":
        sw = f"{R}/plasma-sw"
        env.update({"PATH": f"{sw}/bin:{R}/dbus/bin:/run/current-system/sw/bin", "XDG_DATA_DIRS": f"{sw}/share",
                    "XDG_CONFIG_DIRS": f"{sw}/etc/xdg", "XDG_CURRENT_DESKTOP": "KDE", "XDG_SESSION_DESKTOP": "KDE",
                    "KDE_FULL_SESSION": "true", "KDE_SESSION_VERSION": "6",
                    "QT_PLUGIN_PATH": f"{sw}/lib/qt-6/plugins", "QML2_IMPORT_PATH": f"{sw}/lib/qt-6/qml",
                    "NIXPKGS_QT6_QML_IMPORT_PATH": f"{sw}/lib/qt-6/qml",
                    "KWIN_COMPOSE": "O2", "KWIN_WAYLAND_NO_PERMISSION_CHECKS": "1"})
        cmd = [f"{R}/kwin/bin/kwin_wayland", "--virtual", "--width", "1920", "--height", "1080",
               "--socket", SOCK, "--no-lockscreen"]
    elif desk == "aizome":
        sw = f"{R}/aizome-sw"
        env.update({"PATH": f"{tk.AIZOME}:{sw}/bin:{R}/dbus/bin:/run/current-system/sw/bin",
                    "XDG_DATA_DIRS": f"{sw}/share", "XDG_CONFIG_DIRS": f"{sw}/etc/xdg",
                    "XDG_CURRENT_DESKTOP": "Aizome", "XDG_SESSION_DESKTOP": "aizome",
                    "AIZOME_WALLPAPER_DAY": "", "AIZOME_WALLPAPER_NIGHT": "",
                    "AIZOME_RENDER_NODE": tk.RENDER_NODE, "AIZOME_FONT_DIR": os.path.realpath(tk.FONTS)})
        env["AIZOME_SHELL_BIN"] = private_bus_shell(
            root, env["DBUS_SESSION_BUS_ADDRESS"], f"{tk.AIZOME}/aizome-shell")
        cmd = [f"{tk.AIZOME}/aizome", "--backend", "headless", "--headless-output", "1920x1080@1",
               "--socket", SOCK, "--no-xwayland"]
    return env, cmd


def client_env(desk, root, home, display):
    """The program's environment: the allowlist of the sway runs, with the
    desktop's own integration (Plasma's Qt platform theme; GNOME's and
    Aizome's data and settings)."""
    env = tk.client_env(f"{root}/rt", display, home)
    if desk == "gnome":
        env.update({"XDG_CURRENT_DESKTOP": "GNOME", "XDG_DATA_DIRS": f"{R}/gnome-sw/share"})
    elif desk == "kde":
        sw = f"{R}/plasma-sw"
        env.update({"XDG_CURRENT_DESKTOP": "KDE", "KDE_FULL_SESSION": "true", "KDE_SESSION_VERSION": "6",
                    "QT_QPA_PLATFORMTHEME": "kde", "QT_PLUGIN_PATH": f"{sw}/lib/qt-6/plugins",
                    # Plasma's Breeze style for Qt Quick Controls needs
                    # Kirigami, which the system path doesn't link (KDE's
                    # own apps bring it in their wrappers).
                    "QML2_IMPORT_PATH": f"{sw}/lib/qt-6/qml:{R}/kirigami-unwrapped/lib/qt-6/qml",
                    "NIXPKGS_QT6_QML_IMPORT_PATH": f"{sw}/lib/qt-6/qml:{R}/kirigami-unwrapped/lib/qt-6/qml",
                    "XDG_DATA_DIRS": f"{sw}/share", "XDG_CONFIG_DIRS": f"{sw}/etc/xdg",
                    "QT_FORCE_STDERR_LOGGING": "1"})
    elif desk == "aizome":
        env.update({"XDG_CURRENT_DESKTOP": "Aizome", "AIZOME_SOCKET": f"{root}/rt/aizome.{SOCK}.sock"})
    return env


class Native:
    def __init__(self, desk, root, log):
        self.desk, self.root = desk, root
        if os.path.exists(root):
            tk.kill_runtime(f"{root}/rt")
            shutil.rmtree(root, ignore_errors=True)
        os.makedirs(f"{root}/rt", mode=0o700)
        os.makedirs(f"{root}/home/.config/glib-2.0/settings")
        with open(f"{root}/home/.config/glib-2.0/settings/keyfile", "w") as f:
            f.write("[org/gnome/desktop/background]\npicture-uri=''\npicture-uri-dark=''\npicture-options='none'\n"
                    "primary-color='#202020'\n\n[org/gnome/shell]\nwelcome-dialog-last-shown-version='999'\n\n"
                    "[org/gnome/desktop/interface]\ncursor-blink=false\n")
        open(f"{root}/home/.config/gnome-initial-setup-done", "w").write("yes")
        with open(f"{root}/system-bus.conf", "w") as f:
            f.write(SYSTEM_BUS_CONF.format(path=f"{root}/rt/system_bus"))
        with open(f"{root}/inner.sh", "w") as f:
            f.write(INNER)
        self.env, cmd = session(desk, root)
        devs = ["--dev-bind", tk.RENDER_NODE, tk.RENDER_NODE]
        if os.path.exists("/dev/udmabuf"):
            devs += ["--dev-bind", "/dev/udmabuf", "/dev/udmabuf"]
        sandbox = [tk.TASKSET, "-c", tk.CPU_COMP, BWRAP, "--dev-bind", "/", "/", "--dev", "/dev", *devs,
                   "--tmpfs", "/dev/shm", "--proc", "/proc",
                   "--tmpfs", "/tmp", "--bind", "/tmp/claude-1000", "/tmp/claude-1000",
                   "--perms", "1777", "--tmpfs", "/tmp/.X11-unix",
                   "--tmpfs", "/run/user/1000", "--tmpfs", "/run/dbus", "--tmpfs", "/run/systemd/journal",
                   "--unshare-pid", "--unshare-net", "--die-with-parent", "--chdir", f"{root}/home",
                   "sh", f"{root}/inner.sh", *cmd]
        self.proc = subprocess.Popen(sandbox, env=self.env, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                                     stderr=open(log, "w"), start_new_session=True)

    def wait(self, timeout=40):
        t0 = time.monotonic()
        while time.monotonic() - t0 < timeout:
            if os.path.exists(f"{self.root}/rt/{SOCK}") and self.pids():
                time.sleep(3 if self.desk != "aizome" else 4)
                return True
            if self.proc.poll() is not None:
                return False
            time.sleep(0.2)
        return False

    def pids(self):
        name = COMPOSITOR[self.desk]
        return [p for p in tk.descendants(self.proc.pid) if tk.argv0(p) == name]

    def ns_pid(self):
        """A process inside the sandbox's namespaces: bwrap's child."""
        kids = [p for p in tk.descendants(self.proc.pid) if p != self.proc.pid]
        for p in kids:
            if tk.argv0(p) in ("sh", "dbus-daemon", COMPOSITOR[self.desk]):
                return p
        return kids[0]

    def run(self, argv, env, log=None, cpus=None, stdin=None, stdout=None):
        """Start argv inside the sandbox's namespaces."""
        pre = [NSENTER, "-t", str(self.ns_pid()), "-U", "-p", "-n", "--preserve-credentials", "--"]
        if cpus:
            pre += [tk.TASKSET, "-c", cpus]
        return subprocess.Popen(pre + argv, env=env, stdin=stdin or subprocess.DEVNULL,
                                stdout=stdout or (open(log, "w") if log else subprocess.DEVNULL),
                                stderr=open(log + ".err", "w") if log else subprocess.DEVNULL, start_new_session=True,
                                text=True, bufsize=1)

    def stop(self):
        try:
            os.killpg(self.proc.pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
        try:
            self.proc.wait(8)
        except subprocess.TimeoutExpired:
            os.killpg(self.proc.pid, signal.SIGKILL)
            self.proc.wait()
        tk.kill_runtime(f"{self.root}/rt")


class Input:
    """Each desktop's way to press a key."""

    def __init__(self, nat, out):
        self.nat, self.desk = nat, nat.desk
        if self.desk == "kde":
            env = dict(nat.env, WAYLAND_DISPLAY=SOCK)
            self.p = nat.run([f"{tk.TOOLS}/kwin-input"], env, log=f"{out}/input.log", stdin=subprocess.PIPE,
                             stdout=subprocess.PIPE)
            line = self.p.stdout.readline()
            assert "ready" in line, line
        elif self.desk == "gnome":
            self.eval("global._tkKeys = Clutter.get_default_backend().get_default_seat()"
                      ".create_virtual_device(Clutter.InputDeviceType.KEYBOARD_DEVICE); 'ok'")

    def eval(self, js):
        js = "(() => { const Clutter = imports.gi.Clutter; const GLib = imports.gi.GLib; return " + js + "; })()"
        r = subprocess.run([GDBUS, "call", "--session", "--dest", "org.gnome.Shell", "--object-path",
                            "/org/gnome/Shell", "--method", "org.gnome.Shell.Eval", js],
                           env=self.nat.env, capture_output=True, text=True, timeout=20)
        return r.stdout.strip()

    def key(self, code):
        """Press and release; returns when it was sent (CLOCK_MONOTONIC ns)."""
        if self.desk == "kde":
            self.p.stdin.write(f"key {code}\n")
            self.p.stdin.flush()
            return json.loads(self.p.stdout.readline())["inject"]
        if self.desk == "gnome":
            out = self.eval(f"(() => {{ const t = GLib.get_monotonic_time(); "
                            f"global._tkKeys.notify_key(t, {code}, Clutter.KeyState.PRESSED); "
                            f"global._tkKeys.notify_key(t, {code}, Clutter.KeyState.RELEASED); return String(t); }})()")
            # (true, '"123"')
            digits = "".join(ch for ch in out.split(",", 1)[-1] if ch.isdigit())
            return int(digits) * 1000 if digits else None
        if self.desk == "aizome":
            name = str(code)  # an evdev code
            msg = json.dumps({"request": "synthesize", "events": [{"type": "key", "key": name, "pressed": True},
                                                                   {"type": "key", "key": name, "pressed": False}]})
            s = socket.socket(socket.AF_UNIX)
            s.connect(f"{self.nat.root}/rt/aizome.{SOCK}.sock")
            t = tk.now_ns()
            s.sendall(msg.encode() + b"\n")
            s.recv(4096)
            s.close()
            return t

    def plan(self, plan):
        """GNOME: press every key of `plan` ([(code, ms before the next)]) from
        a GLib timer inside the shell, so one Shell.Eval starts them all
        (an Eval a key would cost GNOME Shell JavaScript and D-Bus work a
        key). Returns when each was sent (ns), once all have been."""
        js = ("(() => { const d = global._tkKeys; const plan = " + json.dumps(plan) + "; global._tkT = []; let i = 0; "
              "const step = () => { if (i >= plan.length) return GLib.SOURCE_REMOVE; const [code, delay] = plan[i++]; "
              "const t = GLib.get_monotonic_time(); d.notify_key(t, code, Clutter.KeyState.PRESSED); "
              "d.notify_key(t, code, Clutter.KeyState.RELEASED); global._tkT.push(t); "
              "GLib.timeout_add(GLib.PRIORITY_DEFAULT, delay, step); return GLib.SOURCE_REMOVE; }; "
              "GLib.timeout_add(GLib.PRIORITY_DEFAULT, 10, step); return 'started'; })()")
        self.eval(js)
        time.sleep(sum(x[1] for x in plan) / 1000 + 0.5)
        for _ in range(20):
            # gdbus prints (true, '[...]').
            out = self.eval("JSON.stringify(global._tkT)")
            got = json.loads(out[out.index("'") + 1:out.rindex("'")]) if "'" in out else []
            # Eval answers with its result as JSON: here a JSON string.
            if isinstance(got, str):
                got = json.loads(got)
            if len(got) >= len(plan):
                return [t * 1000 for t in got]
            time.sleep(0.2)
        return [t * 1000 for t in got]

    def stop(self):
        if self.desk == "kde":
            try:
                self.p.stdin.write("quit\n")
                self.p.stdin.flush()
                self.p.wait(2)
            except Exception:
                pass
            try:
                os.killpg(self.p.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("desk")
    ap.add_argument("subject")
    ap.add_argument("scenario")
    ap.add_argument("out")
    ap.add_argument("--keys", type=int, default=150)
    ap.add_argument("--rate", type=float, default=0)
    ap.add_argument("--seconds", type=float, default=10)
    ap.add_argument("--args", default="")
    ap.add_argument("--seed", type=int, default=1)
    ap.add_argument("--no-proxy", action="store_true")
    a = ap.parse_args()
    os.makedirs(a.out, exist_ok=True)
    rnd = random.Random(a.seed)
    result = dict(desktop=a.desk, subject=a.subject, scenario=a.scenario, args=a.args, load_before=tk.loadavg(),
                  started=time.time())
    tk.fonts_conf(f"{tk.TK}/fontconfig/fonts.conf")
    root = f"{tk.TK}/n-{a.desk}"
    nat = app = px = inp = ppt = None
    try:
        nat = Native(a.desk, root, f"{a.out}/sandbox.log")
        if not nat.wait():
            raise RuntimeError("compositor didn't start")
        comp = nat.pids()
        result["compositor_pids"] = {p: tk.argv0(p) for p in comp}
        ppt = tk.Group([f"{tk.TOOLS}/ppt-sampler", "10", f"{a.out}/ppt.tsv"], {}, cpus=tk.CPU_HARNESS)
        session_all = lambda: [p for p in tk.descendants(nat.proc.pid)]  # noqa: E731
        b0 = tk.sample(comp), tk.sample(session_all())
        time.sleep(3)
        b1 = tk.sample(comp), tk.sample(session_all())
        result["comp_baseline"] = tk.diff(b0[0], b1[0])
        result["session_baseline"] = tk.diff(b0[1], b1[1])
        result["baseline_window"] = [b0[0]["t"], b1[0]["t"]]
        display = SOCK
        if not a.no_proxy:
            px = nat.run([f"{tk.TOOLS}/wltrace", "tkp", f"{a.out}/trace.log"],
                         {"XDG_RUNTIME_DIR": f"{root}/rt", "WAYLAND_DISPLAY": SOCK}, cpus=tk.CPU_TOOLS)
            for _ in range(100):
                if os.path.exists(f"{root}/rt/tkp"):
                    break
                time.sleep(0.02)
            display = "tkp"
        home = f"{tk.TK}/home/{a.subject}-{a.desk}"
        tk.prepare_home(home)
        argv, app_id, family = tk.subjects()[a.subject]
        env = client_env(a.desk, root, home, display)
        app = nat.run(argv + a.args.split(), env, log=f"{a.out}/app.log", cpus=tk.CPU_APP)
        time.sleep(5)
        apps = lambda: [p for p in tk.descendants(app.pid) if p != app.pid]  # noqa: E731
        result["app_procs"] = {p: tk.argv0(p) for p in apps()}
        if a.scenario == "keys":
            inp = Input(nat, a.out)
            for i in range(10):  # warm-up
                inp.key(KEY_A if i < 5 else KEY_BACKSPACE)
                time.sleep(0.1)
            time.sleep(1)
            i0 = tk.sample(apps()), tk.sample(comp)
            time.sleep(5)
            i1 = tk.sample(apps()), tk.sample(comp)
            result["idle"] = dict(app=tk.diff(i0[0], i1[0]), comp=tk.diff(i0[1], i1[1]))
            plan = [(KEY_A if (i // 10) % 2 == 0 else KEY_BACKSPACE,
                     round(1000 / a.rate) if a.rate else rnd.randint(80, 130)) for i in range(a.keys)]
            t0 = tk.sample(apps()), tk.sample(comp)
            injects = []
            if a.desk == "gnome":
                injects = inp.plan(plan)
            else:
                for code, gap in plan:
                    t_send = time.monotonic()
                    injects.append(inp.key(code))
                    time.sleep(max(0, gap / 1000 - (time.monotonic() - t_send)))
            time.sleep(0.3)
            t1 = tk.sample(apps()), tk.sample(comp)
            result["typing"] = dict(app=tk.diff(t0[0], t1[0]), comp=tk.diff(t0[1], t1[1]), keys=a.keys,
                                    t0=t0[0]["t"], t1=t1[0]["t"])
            result["idle_window"] = [i0[0]["t"], i1[0]["t"]]
            result["injects"] = injects
        else:
            t0 = tk.sample(apps()), tk.sample(comp)
            time.sleep(a.seconds)
            t1 = tk.sample(apps()), tk.sample(comp)
            result["window_stats"] = dict(app=tk.diff(t0[0], t1[0]), comp=tk.diff(t0[1], t1[1]),
                                          t0=t0[0]["t"], t1=t1[0]["t"])
        result["load_after"] = tk.loadavg()
    finally:
        if inp:
            inp.stop()
        for p in (app, px):
            if p:
                try:
                    os.killpg(p.pid, signal.SIGTERM)
                    p.wait(3)
                except Exception:
                    try:
                        os.killpg(p.pid, signal.SIGKILL)
                    except ProcessLookupError:
                        pass
        if ppt:
            ppt.stop()
        if nat:
            nat.stop()
    with open(f"{a.out}/result.json", "w") as f:
        json.dump(result, f)


if __name__ == "__main__":
    main()
