#!/usr/bin/env python3
"""Aizome vs GNOME vs Plasma in a small VM (docs/performance/desktops.md).

Boots the systems of default.nix in QEMU/KVM (2 vCPUs, 4 GiB, virtio-gpu
without 3D at 1920x1080, disks with cache=none), pinned to fixed host CPUs,
and measures what the person sees through QMP screendumps and what the guest
does through a root shell on a virtio-serial port (instrument.nix).

    bench.py setup VARIANT          first boot: first login, dismiss first-run
                                    windows, open every app once, save the
                                    disk as the golden image; then capture the
                                    reference greeter/desktop frames
    bench.py trial VARIANT TAG      one measured boot (run it in lock-measure)
    bench.py shell VARIANT          boot a throwaway copy and keep it running

Run with the `host` environment of default.nix on PATH (QEMU, erofs-utils,
e2fsprogs, Python with NumPy); see run.sh.
"""
import argparse
import base64
import json
import lzma
import os
import random
import re
import shlex
import shutil
import signal
import socket
import subprocess
import sys
import time
from pathlib import Path

import numpy as np

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[2]
ROOT = Path(os.environ.get("DESKBENCH_ROOT", f"{os.environ.get('TMPDIR') or '/tmp'}/aizome-perf/desktops"))
CPUS = os.environ.get("DESKBENCH_CPUS", "2,3,10,11")
# tests/performance/powermeter.c, built (docs/performance/README.md, Power).
POWERMETER = os.environ.get("POWERMETER", f"{os.environ.get('TMPDIR') or '/tmp'}/aizome-perf/bin/powermeter")
W, H = 1920, 1080
PIXELS = W * H
# A pixel has changed when a channel moved by more than this (0-255).
PIXEL_THRESHOLD = 24
# A frame differs significantly from the previous one when more pixels than
# this changed: a blinking text cursor or a clock's minute digits stay below.
SIGNIFICANT = 2500
PASSWORD = "demo"
IMAGE = "/run/current-system/sw/share/backgrounds/tatami/hintersee.jpg"
PDF = "/home/demo/Documents/fixture.pdf"

# Per desktop: how to recognise its login screen and session in the guest,
# how its login screen takes the password, and its applications.
DESKTOPS = {
    "baseline": {"greeter": None, "session": None},
    "aizome": {
        "greeter": "pgrep -u greeter -x aizome-greeter",
        "session": "pgrep -u demo -x aizome-shell",
        "login": ["password"],
        "apps": [
            ("terminal", "Hotaru", ["aizome-hotaru"]),
            ("files", "Hako", ["aizome-hako"]),
            ("settings", "Settei", ["aizome-settei"]),
            ("editor", "Sumi", ["aizome-sumi"]),
            ("calculator", "Soroban", ["aizome-soroban"]),
            ("image", "Shashin", ["aizome-shashin", IMAGE]),
            ("pdf", "Shiori", ["aizome-shiori", PDF]),
        ],
    },
    "gnome": {
        "greeter": "pgrep -u gdm-greeter -f 'gnome-shell( |$)'",
        "session": "pgrep -u demo -f 'gnome-shell( |$)'",
        # GDM lists the user first: Enter picks them, then the password.
        "login": ["ret", 2.0, "password"],
        "apps": [
            ("terminal", "Console", ["kgx"]),
            ("files", "Files (Nautilus)", ["nautilus"]),
            ("settings", "Settings", ["gnome-control-center"]),
            ("editor", "Text Editor", ["gnome-text-editor"]),
            ("calculator", "Calculator", ["gnome-calculator"]),
            ("image", "Image Viewer (Loupe)", ["loupe", IMAGE]),
            ("pdf", "Document Viewer (Papers)", ["papers", PDF]),
        ],
    },
    "plasma": {
        "greeter": "systemctl is-active display-manager && pgrep -f -i 'plasma.?login.*greet|kwin_wayland.*plasma.?login'",
        "session": "pgrep -u demo -f 'plasmashell( |$)'",
        # Its controls hide when idle: a key wakes them; clear what it typed.
        "login": ["spc", 1.0, "ctrl-a", "backspace", "password"],
        "apps": [
            ("terminal", "Konsole", ["konsole"]),
            ("files", "Dolphin", ["dolphin"]),
            ("settings", "System Settings", ["systemsettings"]),
            ("editor", "KWrite", ["kwrite"]),
            ("image", "Gwenview", ["gwenview", IMAGE]),
            ("pdf", "Okular", ["okular", PDF]),
        ],
    },
}


def log(*args):
    print(time.strftime("%H:%M:%S"), *args, flush=True)


# ---------------------------------------------------------------- nix paths


def nix_paths(variant):
    attrs = [f"{variant}.toplevel", f"{variant}.closure", f"{variant}.versions", f"{variant}.footprint"]
    out = subprocess.run(
        ["nix", "build", "--impure", "-f", str(HERE), "--no-link", "--print-out-paths", *attrs],
        check=True, capture_output=True, text=True, cwd=REPO,
    ).stdout.split()
    return dict(zip(["toplevel", "closure", "versions", "footprint"], out))


def store_image(variant, paths):
    """The system closure as an erofs image, as nixpkgs' useNixStoreImage
    makes it, built once per toplevel."""
    tag = Path(paths["toplevel"]).name.split("-")[0]
    image = ROOT / "img" / variant / f"store-{tag}.img"
    if image.exists():
        return image
    image.parent.mkdir(parents=True, exist_ok=True)
    for old in image.parent.glob("store-*.img"):
        old.unlink()
    log("building store image", image)
    tmp = image.with_suffix(".tmp")
    tar = subprocess.Popen(
        ["tar", "--create", "--absolute-names", "--verbatim-files-from",
         "--transform", "flags=rSh;s|/nix/store/||",
         "--transform", r"flags=rSh;s|~nix~case~hack~[[:digit:]]\+||g",
         "--files-from", paths["closure"] + "/store-paths"],
        stdout=subprocess.PIPE)
    subprocess.run(
        ["mkfs.erofs", "--quiet", "--force-uid=0", "--force-gid=0", "-L", "nix-store",
         "-U", "eb176051-bd15-49b7-9e6b-462e0b467019", "-T", "0", "--hard-dereference", "--tar=f", str(tmp)],
        stdin=tar.stdout, check=True)
    tar.stdout.close()
    if tar.wait() != 0:
        raise RuntimeError("tar failed")
    tmp.rename(image)
    return image


def new_root(path, size="16G"):
    raw = path.with_suffix(".raw")
    subprocess.run(["truncate", "-s", size, str(raw)], check=True)
    subprocess.run(["mkfs.ext4", "-q", "-L", "nixos", str(raw)], check=True)
    subprocess.run(["qemu-img", "convert", "-f", "raw", "-O", "qcow2", str(raw), str(path)], check=True)
    raw.unlink()


def overlay(golden, path):
    subprocess.run(["qemu-img", "create", "-q", "-f", "qcow2", "-b", str(golden), "-F", "qcow2", str(path)], check=True)


def new_swap(path, mib):
    if path.exists():
        path.unlink()
    subprocess.run(["truncate", "-s", f"{mib}M", str(path)], check=True)
    subprocess.run(["mkswap", "-q", "-L", "swap", str(path)], check=True)


# ---------------------------------------------------------------- QMP and agent


class QMP:
    def __init__(self, path, timeout=30):
        self.sock = connect(path, timeout)
        self.buf = b""
        self.events = []
        self._read()  # greeting
        self.cmd("qmp_capabilities")

    def _read(self, timeout=60):
        self.sock.settimeout(timeout)
        while b"\n" not in self.buf:
            chunk = self.sock.recv(65536)
            if not chunk:
                raise EOFError("QMP closed")
            self.buf += chunk
        line, self.buf = self.buf.split(b"\n", 1)
        return json.loads(line)

    def cmd(self, name, timeout=60, **args):
        msg = {"execute": name}
        if args:
            msg["arguments"] = args
        self.sock.sendall(json.dumps(msg).encode() + b"\n")
        while True:
            reply = self._read(timeout)
            if "event" in reply:
                reply["host_time"] = time.monotonic()
                self.events.append(reply)
                continue
            if "error" in reply:
                raise RuntimeError(f"QMP {name}: {reply['error']}")
            return reply.get("return")


class Agent:
    """The guest's root bash on the perf.agent port (instrument.nix)."""

    PRELUDE = r"""
export PATH=/run/wrappers/bin:/run/current-system/sw/bin:$PATH LC_ALL=C
__r() { local o rc; o=$(bash -c "$(printf '%s' "$2" | base64 -d)" 2>&1 </dev/null); rc=$?; printf '\n@@%s %s %s\n' "$1" "$rc" "$(printf '%s' "$o" | base64 -w0)"; }
"""

    def __init__(self, path, timeout=300):
        self.path = path
        self.sock = connect(path, timeout)
        self.buf = b""
        self.n = 0
        self.sock.sendall(self.PRELUDE.encode())
        deadline = time.monotonic() + timeout
        while True:
            try:
                rc, out = self.run("echo ok", timeout=5)
                if out.strip() == "ok":
                    break
            except TimeoutError:
                pass
            if time.monotonic() > deadline:
                raise TimeoutError("agent did not answer")
            self.sock.sendall(self.PRELUDE.encode())

    def run(self, command, timeout=120, check=False):
        self.n += 1
        ident = f"{os.getpid()}-{self.n}"
        payload = base64.b64encode(command.encode()).decode()
        self.sock.sendall(f"__r {ident} {payload}\n".encode())
        deadline = time.monotonic() + timeout
        marker = f"@@{ident} ".encode()
        while True:
            idx = self.buf.find(marker)
            if idx >= 0:
                end = self.buf.find(b"\n", idx)
                if end >= 0:
                    line = self.buf[idx:end].decode()
                    self.buf = self.buf[end + 1:]
                    parts = line.split(" ", 2)
                    rc = int(parts[1])
                    out = base64.b64decode(parts[2]).decode(errors="replace") if len(parts) > 2 else ""
                    if check and rc != 0:
                        raise RuntimeError(f"guest command failed ({rc}): {command}\n{out}")
                    return rc, out
            left = deadline - time.monotonic()
            if left <= 0:
                raise TimeoutError(command)
            self.sock.settimeout(left)
            try:
                chunk = self.sock.recv(1 << 20)
            except socket.timeout:
                raise TimeoutError(command)
            if not chunk:
                raise EOFError("agent closed")
            self.buf += chunk

    def ok(self, command, timeout=120):
        return self.run(command, timeout, check=True)[1]

    def user(self, command, timeout=120, check=True):
        full = ("runuser -u demo -- env XDG_RUNTIME_DIR=/run/user/1000 "
                "DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus sh -c " + shlex.quote(command))
        return self.run(full, timeout, check=check)[1]


def connect(path, timeout):
    deadline = time.monotonic() + timeout
    while True:
        s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        try:
            s.connect(str(path))
            return s
        except (FileNotFoundError, ConnectionRefusedError):
            s.close()
            if time.monotonic() > deadline:
                raise
            time.sleep(0.05)


# ---------------------------------------------------------------- guest shell helpers

GUEST_LIB = r"""
snap() {
  local d p k v r pss pa pf ps sw swp rss comm uid ppid cg a0
  for d in /proc/[0-9]*; do
    p=${d#/proc/}
    pss=; pa=0; pf=0; ps=0; sw=0; swp=0; rss=0
    while read -r k v r; do
      case $k in Rss:) rss=$v;; Pss:) pss=$v;; Pss_Anon:) pa=$v;; Pss_File:) pf=$v;; Pss_Shmem:) ps=$v;; Swap:) sw=$v;; SwapPss:) swp=$v;; esac
    done < $d/smaps_rollup 2>/dev/null
    [ -n "$pss" ] || continue
    uid=; ppid=; comm=
    while read -r k v r; do
      case $k in Name:) comm="$v${r:+ $r}";; PPid:) ppid=$v;; Uid:) uid=$v;; esac
    done < $d/status 2>/dev/null
    cg=; read -r cg < $d/cgroup 2>/dev/null; cg=${cg#0::}
    a0=; read -r -d '' a0 < $d/cmdline 2>/dev/null
    printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$p" "$ppid" "$uid" "$comm" "$cg" "$rss" "$pss" "$pa" "$pf" "$ps" "$sw" "$swp" "${a0##*/}"
  done
}
tasks() {
  local t k v r s rest comm vol nv cg
  for t in /proc/[0-9]*/task/[0-9]*; do
    vol=0; nv=0
    while read -r k v r; do
      case $k in voluntary_ctxt_switches:) vol=$v;; nonvoluntary_ctxt_switches:) nv=$v;; esac
    done < $t/status 2>/dev/null
    read -r s < $t/stat 2>/dev/null || continue
    comm=${s#*(}; comm=${comm%)*}; rest=${s##*) }
    set -- $rest
    cg=; read -r cg < ${t%/task/*}/cgroup 2>/dev/null; cg=${cg#0::}
    printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "${t#/proc/}" "$comm" "$vol" "$nv" "${12}" "${13}" "$cg"
  done
}
cgmem() {
  local f
  for f in /sys/fs/cgroup/*.slice/memory.current /sys/fs/cgroup/user.slice/*/memory.current \
           /sys/fs/cgroup/user.slice/*/*/memory.current /sys/fs/cgroup/user.slice/*/user@*.service/*/memory.current \
           /sys/fs/cgroup/system.slice/*/memory.current; do
    [ -r "$f" ] && printf '%s\t%s\t%s\n' "${f#/sys/fs/cgroup/}" "$(cat $f)" "$(cat ${f%/memory.current}/memory.swap.current 2>/dev/null)"
  done
}
"""


class VM:
    def __init__(self, variant, run_dir, root, store, swap, toplevel, mem=4096, cpus=2, rtc=None):
        self.variant = variant
        self.dir = Path(run_dir)
        self.dir.mkdir(parents=True, exist_ok=True)
        self.sockdir = Path(f"/tmp/dbs-{variant}")  # short: QMP and serial sockets
        shutil.rmtree(self.sockdir, ignore_errors=True)
        self.sockdir.mkdir(parents=True)
        self.shm = Path(f"/dev/shm/aizome-dbs-{variant}")
        self.shm.mkdir(parents=True, exist_ok=True)
        self.dump = self.shm / "screen.ppm"
        params = Path(toplevel, "kernel-params").read_text().strip()
        self.argv = [
            "taskset", "-c", CPUS,
            "qemu-system-x86_64", "-name", f"sekka-{variant}",
            "-machine", "pc,accel=kvm", "-cpu", "host", "-smp", str(cpus), "-m", str(mem),
            "-nodefaults", "-no-user-config", "-display", "none",
            "-device", "virtio-rng-pci",
            "-netdev", "user,id=n0,restrict=on", "-device", "virtio-net-pci,netdev=n0",
            "-drive", f"if=none,id=root,file={root},format=qcow2,cache=none,werror=report",
            "-device", "virtio-blk-pci,drive=root,serial=root",
            "-drive", f"if=none,id=store,file={store},format=raw,readonly=on,cache=none",
            "-device", "virtio-blk-pci,drive=store,serial=store",
            "-drive", f"if=none,id=swap,file={swap},format=raw,cache=none",
            "-device", "virtio-blk-pci,drive=swap,serial=swap",
            # virtio-gpu with VGA compatibility (QEMU's -vga virtio, libvirt's
            # default video for Linux guests), 2D only: no virgl. As a VGA-class
            # device it is the boot VGA, which GDM waits up to 10 s for.
            "-vga", "none", "-device", f"virtio-vga,xres={W},yres={H}",
            "-usb", "-device", "usb-tablet",
            "-kernel", f"{toplevel}/kernel", "-initrd", f"{toplevel}/initrd",
            "-append", f"{params} init={toplevel}/init console=ttyS0,115200 console=tty0",
            "-serial", f"file:{self.dir}/serial.log",
            "-qmp", f"unix:{self.sockdir}/qmp,server=on,wait=off",
            "-chardev", f"socket,id=agent,path={self.sockdir}/agent,server=on,wait=off",
            "-device", "virtio-serial-pci", "-device", "virtserialport,chardev=agent,name=perf.agent",
            "-rtc", f"base={rtc}" if rtc else "base=utc",
        ]
        self.proc = None
        self.qmp = None
        self.agent = None

    def start(self):
        (self.dir / "qemu-argv.json").write_text(json.dumps(self.argv, indent=1))
        self.t0 = time.monotonic()
        self.proc = subprocess.Popen(self.argv, stdin=subprocess.DEVNULL,
                                     stdout=open(self.dir / "qemu.log", "w"), stderr=subprocess.STDOUT)
        self.qmp = QMP(self.sockdir / "qmp")
        return self.t0

    def pid(self):
        return self.proc.pid

    def connect_agent(self, timeout=300):
        if self.agent is None:
            self.agent = Agent(self.sockdir / "agent", timeout)
            # Defined in the agent's own shell and exported to each command's.
            self.agent.sock.sendall((GUEST_LIB + "\nexport -f snap tasks cgmem\n").encode())
            self.agent.ok("type snap tasks cgmem >/dev/null")
        return self.agent

    def grab(self):
        for attempt in range(50):
            try:
                self.qmp.cmd("screendump", filename=str(self.dump))
                break
            except RuntimeError:
                # No display surface yet (QEMU just started).
                time.sleep(0.1)
        t = time.monotonic()
        data = self.dump.read_bytes()
        # P6\n<w> <h>\n255\n
        parts = data.split(b"\n", 3)
        w, h = map(int, parts[1].split())
        arr = np.frombuffer(parts[3], dtype=np.uint8, count=w * h * 3).reshape(h, w, 3)
        return t, arr

    def png(self, name):
        path = self.dir / f"{name}.png"
        self.qmp.cmd("screendump", filename=str(path), format="png")
        return path

    def key(self, *codes, hold=0.03):
        events = lambda down, seq: [{"type": "key", "data": {"down": down, "key": {"type": "qcode", "data": c}}} for c in seq]
        self.qmp.cmd("input-send-event", events=events(True, codes))
        time.sleep(hold)
        self.qmp.cmd("input-send-event", events=events(False, list(reversed(codes))))

    def type(self, text):
        names = {" ": "spc", "\n": "ret", ".": "dot", "-": "minus", "/": "slash"}
        for ch in text:
            self.key(names.get(ch, ch))
            time.sleep(0.03)

    def guest_clock_offset(self):
        """guest CLOCK_MONOTONIC minus host CLOCK_MONOTONIC (s), best of 5."""
        best = None
        for _ in range(5):
            a = time.monotonic()
            out = self.agent.ok("perf-util clock")
            b = time.monotonic()
            g = int(out.strip()) / 1e9
            if best is None or b - a < best[0]:
                best = (b - a, g - (a + b) / 2)
        return {"offset_s": best[1], "rtt_s": best[0]}

    def shutdown(self, timeout=90):
        if self.proc is None or self.proc.poll() is not None:
            return
        try:
            if self.agent:
                self.agent.sock.sendall(b"systemctl poweroff\n")
            else:
                self.qmp.cmd("system_powerdown")
        except Exception:
            pass
        try:
            self.proc.wait(timeout)
        except subprocess.TimeoutExpired:
            log("poweroff timed out; quitting QEMU")
            try:
                self.qmp.cmd("quit")
            except Exception:
                self.proc.kill()
            self.proc.wait(30)

    def kill(self):
        if self.proc and self.proc.poll() is None:
            self.proc.kill()
            self.proc.wait()


# ---------------------------------------------------------------- screen observation


def ndiff(a, b, step=1):
    if b is None or a.shape != b.shape:
        return PIXELS
    if step > 1:
        a = a[::step, ::step]
        b = b[::step, ::step]
    if np.array_equal(a, b):
        return 0
    d = np.abs(a.astype(np.int16) - b.astype(np.int16)).max(axis=2)
    return int(np.count_nonzero(d > PIXEL_THRESHOLD)) * step * step


def watch(vm, t0, *, ref=None, ref_tol=0.02, quiet=3.0, timeout=180.0, interval=0.1, base=None,
          min_from_base=0, cond=None, cond_every=2.0, need_change=False):
    """Poll screendumps until the screen has settled.

    Settled: no frame-to-frame change above SIGNIFICANT for `quiet` seconds,
    while the frame matches `ref` (within ref_tol of the pixels), differs from
    `base` by at least min_from_base pixels, and cond() holds (checked every
    cond_every seconds). Returns times relative to t0: the first significant
    change, the last one (when the settled picture appeared), the first frame
    matching ref, and per-frame statistics."""
    prev = None
    frames = []
    first = last = first_match = None
    cond_ok = cond is None
    cond_checked = 0.0
    final = None
    while True:
        tq = time.monotonic()
        t, a = vm.grab()
        dp = ndiff(a, prev) if prev is not None else 0
        dr = ndiff(a, ref, 2) if ref is not None else None
        db = ndiff(a, base, 2) if base is not None else None
        if prev is not None and dp > SIGNIFICANT:
            last = t
            if first is None:
                first = t
        prev = a
        match = True
        if ref is not None:
            match = dr <= ref_tol * PIXELS
            if match and first_match is None:
                first_match = t
            elif not match:
                first_match = None
        if base is not None and db < min_from_base:
            match = False
        if need_change and last is None:
            match = False
        if cond is not None and not cond_ok and t - cond_checked >= cond_every:
            cond_checked = t
            cond_ok = bool(cond())
        frames.append([round(t - t0, 4), dp, dr, db])
        since = t - (last if last is not None else t0)
        if match and cond_ok and since >= quiet:
            final = a
            break
        if t - t0 > timeout:
            return {"timeout": True, "frames": frames, "first_change": rel(first, t0), "last_change": rel(last, t0),
                    "final": a}
        wait = interval - (time.monotonic() - tq)
        if wait > 0:
            time.sleep(wait)
    return {"timeout": False, "first_change": rel(first, t0), "settled": rel(last, t0) if last else 0.0,
            "first_match": rel(first_match, t0), "frames": frames, "final": final,
            "interval": interval, "quiet": quiet}


def rel(t, t0):
    return None if t is None else round(t - t0, 4)


def save_ppm(arr, path):
    with open(path, "wb") as f:
        f.write(f"P6\n{arr.shape[1]} {arr.shape[0]}\n255\n".encode())
        f.write(arr.tobytes())


def load_ppm(path):
    data = Path(path).read_bytes()
    parts = data.split(b"\n", 3)
    w, h = map(int, parts[1].split())
    return np.frombuffer(parts[3], dtype=np.uint8, count=w * h * 3).reshape(h, w, 3).copy()


# ---------------------------------------------------------------- host measurements


def qemu_cpu(pid):
    """Per-thread CPU seconds of the QEMU process (utime+stime)."""
    tick = os.sysconf("SC_CLK_TCK")
    out = {}
    for task in Path(f"/proc/{pid}/task").iterdir():
        try:
            s = (task / "stat").read_text()
            comm = (task / "comm").read_text().strip()
        except OSError:
            continue
        rest = s.rsplit(")", 1)[1].split()
        out[f"{task.name}:{comm}"] = (int(rest[11]) + int(rest[12])) / tick
    return out


def loadavg():
    return Path("/proc/loadavg").read_text().split()[:3]


def wait_quiet_host(limit=1.5, timeout=600):
    start = time.monotonic()
    while float(loadavg()[0]) >= limit and time.monotonic() - start < timeout:
        time.sleep(5)
    return {"waited_s": round(time.monotonic() - start, 1), "loadavg": loadavg()}


class Power:
    def __init__(self, path):
        self.path = Path(path)
        self.proc = subprocess.Popen([POWERMETER, "-i", "100", "-o", str(self.path.with_suffix(".tsv")),
                                      "-j", str(self.path)], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

    def stop(self):
        self.proc.send_signal(signal.SIGINT)
        self.proc.wait(30)
        try:
            return json.loads(self.path.read_text())
        except Exception as e:
            return {"error": str(e)}


# ---------------------------------------------------------------- desktop flows


def login(vm, variant):
    """Type the password the way the desktop's login screen takes it.
    Returns the host time the final Enter was pressed."""
    for step in DESKTOPS[variant]["login"]:
        if isinstance(step, float):
            time.sleep(step)
        elif step == "password":
            vm.type(PASSWORD)
            time.sleep(0.3)
        else:
            vm.key(*step.split("-"))
            time.sleep(0.2)
    t = time.monotonic()
    vm.key("ret")
    return t


def wait_guest(agent, command, timeout=240):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if agent.run(command, 30)[0] == 0:
            return True
        time.sleep(1)
    return False


def memory(vm, out, name):
    a = vm.agent
    (out / f"mem-{name}.tsv").write_text(a.ok("snap", 120))
    (out / f"meminfo-{name}.txt").write_text(a.ok("cat /proc/meminfo; echo; cat /proc/pressure/memory; echo; cat /proc/vmstat | grep -E '^(pswp|zswp|workingset_refault)'"))
    (out / f"cgmem-{name}.tsv").write_text(a.ok("cgmem"))


def meminfo_summary(text):
    kv = {}
    for line in text.splitlines():
        m = re.match(r"(\w+):\s+(\d+)", line)
        if m:
            kv[m.group(1)] = int(m.group(2))
    return {"used_mib": round((kv["MemTotal"] - kv["MemAvailable"]) / 1024, 1),
            "swap_used_mib": round((kv["SwapTotal"] - kv["SwapFree"]) / 1024, 1),
            "zswap_mib": round(kv.get("Zswap", 0) / 1024, 1),
            "zswapped_mib": round(kv.get("Zswapped", 0) / 1024, 1),
            "memtotal_mib": round(kv["MemTotal"] / 1024, 1)}


def idle_window(vm, out, seconds=120):
    a = vm.agent
    (out / "idle-tasks-start.tsv").write_text(a.ok("tasks", 120))
    stat0 = a.ok("cat /proc/stat")
    p = Power(out / "idle-power.json")
    cpu0 = qemu_cpu(vm.pid())
    h0 = time.monotonic()
    time.sleep(seconds)
    cpu1 = qemu_cpu(vm.pid())
    h1 = time.monotonic()
    power = p.stop()
    stat1 = a.ok("cat /proc/stat")
    (out / "idle-tasks-end.tsv").write_text(a.ok("tasks", 120))
    (out / "idle-stat.txt").write_text(stat0 + "\n----\n" + stat1)
    return {"host_seconds": round(h1 - h0, 3), "qemu_cpu_start": cpu0, "qemu_cpu_end": cpu1,
            "power": power, "stat": [parse_stat(stat0), parse_stat(stat1)]}


def parse_stat(text):
    d = {}
    for line in text.splitlines():
        f = line.split()
        if not f:
            continue
        if f[0] == "cpu":
            d["cpu"] = list(map(int, f[1:]))
        elif f[0] in ("ctxt", "processes", "btime"):
            d[f[0]] = int(f[1])
        elif f[0] == "intr":
            d["intr"] = int(f[1])
    return d


def journal(vm, out):
    data = vm.agent.ok("journalctl -b -o json --no-pager", 300)
    with lzma.open(out / "journal.json.xz", "wt") as f:
        f.write(data)


def analyze(vm, out, variant):
    a = vm.agent
    target = "multi-user.target" if variant == "baseline" else "display-manager.service"
    text = a.run("systemd-analyze; echo; systemd-analyze critical-chain " + target + "; echo; systemd-analyze blame", 120)[1]
    (out / "systemd-analyze.txt").write_text(text)
    units = a.run("systemctl show display-manager.service multi-user.target graphical.target -p Id "
                  "-p ActiveEnterTimestampMonotonic -p InactiveExitTimestampMonotonic", 60)[1]
    (out / "units.txt").write_text(units)
    m = re.search(r"Startup finished in (.*)", text)
    return m.group(1) if m else None


def systemd_run(agent, unit, argv, extra=""):
    if not argv[0].startswith("/"):
        argv = ["/run/current-system/sw/bin/" + argv[0], *argv[1:]]
    cmd = ("systemd-run --user -M demo@ --quiet --collect --slice=app.slice --unit=" + unit + " " + extra + " -- "
           + " ".join(shlex.quote(x) for x in argv))
    return agent.run(cmd, 60)


def user_systemctl(agent, args):
    return agent.run("systemctl --user -M demo@ " + args, 60)


def probe(vm, out, name, mode, arg, inject=None):
    a = vm.agent
    a.ok("mkdir -p /tmp/perf && chmod 1777 /tmp/perf && rm -f /tmp/perf/" + name + ".*")
    status = f"/tmp/perf/{name}.status"
    rc, msg = systemd_run(a, "perf-" + name, ["wl-probe", mode, str(arg), f"/tmp/perf/{name}.tsv", status])
    if rc:
        return {"error": msg}
    deadline = time.monotonic() + 50
    st = ""
    clicked = None
    while time.monotonic() < deadline:
        st = a.run("cat " + status + " 2>/dev/null")[1].strip()
        if st.startswith(("ready", "not-ready", "no-display", "missing")):
            break
        if st == "mapped" and clicked is None:
            # The compositor showed the window without focusing it: click
            # it, as a person would. Its first frame is pale yellow.
            time.sleep(0.5)
            clicked = click_colour(vm, (0xf2, 0xe8, 0xa0)) or click_colour(vm, (0x1b, 0x2a, 0x6b))
        time.sleep(0.3)
    result = {"status": st, "clicked": clicked}
    if not st.startswith("ready"):
        user_systemctl(a, "stop perf-" + name)
        (out / f"{name}-journal.txt").write_text(a.run("journalctl -b --no-pager -n 40 _UID=1000")[1])
        return result
    if inject:
        count, lo, hi = inject
        rc, msg = a.run(f"uinput-keys {count} {lo} {hi} /tmp/perf/{name}.inject.tsv 2500", 60 + count * hi / 1000)
        result["inject_rc"] = rc
    deadline = time.monotonic() + (arg + 30 if mode == "pacing" else 60)
    while time.monotonic() < deadline:
        st = a.run("cat " + status)[1].strip()
        if st == "done":
            break
        time.sleep(0.5)
    if st != "done":
        user_systemctl(a, "kill --signal=TERM perf-" + name)
        time.sleep(3)
    result["final_status"] = a.run("cat " + status)[1].strip()
    for suffix in ("tsv", "inject.tsv"):
        rc, data = a.run(f"cat /tmp/perf/{name}.{suffix}")
        if rc == 0:
            (out / f"{name}.{suffix}").write_text(data)
    user_systemctl(a, "stop perf-" + name)
    return result


def click_colour(vm, rgb):
    """Click the middle of the region of the screen in exactly this colour."""
    _, a = vm.grab()
    mask = np.all(a == np.array(rgb, dtype=np.uint8), axis=2)
    if mask.sum() < 10000:
        return None
    ys, xs = np.nonzero(mask)
    x, y = int((xs.min() + xs.max()) / 2), int((ys.min() + ys.max()) / 2)
    vm.qmp.cmd("input-send-event", events=[
        {"type": "abs", "data": {"axis": "x", "value": round(x * 32767 / (W - 1))}},
        {"type": "abs", "data": {"axis": "y", "value": round(y * 32767 / (H - 1))}}])
    time.sleep(0.2)
    vm.qmp.cmd("input-send-event", events=[{"type": "btn", "data": {"button": "left", "down": True}}])
    time.sleep(0.05)
    vm.qmp.cmd("input-send-event", events=[{"type": "btn", "data": {"button": "left", "down": False}}])
    return [x, y]


def responsiveness(vm, out, variant, presses=220):
    a = vm.agent
    res = {}
    if variant == "gnome":
        # Leave the overview GNOME opens at login, so the probe is a window
        # on the desktop like the others'.
        vm.key("esc")
        time.sleep(2)
    res["latency_idle"] = probe(vm, out, "latency-idle", "latency", presses, (presses, 120, 250))
    time.sleep(2)
    res["pacing_idle"] = probe(vm, out, "pacing-idle", "pacing", 30 if presses >= 100 else 5)
    time.sleep(2)
    systemd_run(a, "perf-load", ["perf-util", "spin", "2"])
    time.sleep(5)
    res["load_check"] = a.run("top -b -n1 | head -15")[1]
    res["latency_load"] = probe(vm, out, "latency-load", "latency", presses, (presses, 120, 250))
    time.sleep(2)
    res["pacing_load"] = probe(vm, out, "pacing-load", "pacing", 30 if presses >= 100 else 5)
    user_systemctl(a, "stop perf-load")
    time.sleep(5)
    return res


def kill_app(agent, unit, argv):
    user_systemctl(agent, "stop " + unit)
    name = Path(argv[0]).name
    pattern = f"(^|/)\\.?[{name[0]}]{re.escape(name[1:])}(-wrapped)?( |$)"
    agent.run(f"pkill -u demo -f '{pattern}'")
    time.sleep(1.5)


def launch(vm, out, variant, key, argv, n, save_png=False):
    a = vm.agent
    unit = f"perfapp-{key}-{n}"
    pids0 = set(a.ok("ls /proc | grep -E '^[0-9]+$'").split())
    t, base = vm.grab()
    t0 = time.monotonic()
    rc, msg = systemd_run(a, unit, argv)
    t1 = time.monotonic()
    w = watch(vm, t0, base=base, min_from_base=20000, quiet=2.0, timeout=60, interval=0.05)
    res = {"launch_rc": rc, "launch_msg": msg.strip()[:500], "systemd_run_s": round(t1 - t0, 4),
           "first_change": w.get("first_change"), "settled": w.get("settled"), "timeout": w["timeout"],
           "frames": w["frames"]}
    time.sleep(1.0)
    cg = f"/sys/fs/cgroup/user.slice/user-1000.slice/user@1000.service/app.slice/{unit}.service"
    rows = a.ok("snap", 120)
    pids_app = set(a.run(f"cat {cg}/cgroup.procs 2>/dev/null")[1].split())
    res["cgroup_memory_current"] = a.run(f"cat {cg}/memory.current 2>/dev/null")[1].strip()
    app_pss = other_pss = 0
    new = []
    for line in rows.splitlines():
        f = line.split("\t")
        if len(f) < 13 or "perf-agent" in f[4]:
            continue
        # The app's processes: its unit's, and new ones it put in scopes of
        # its own under app.slice (Konsole does, per window and tab).
        if f[0] in pids_app or (f[0] not in pids0 and "/app.slice/" in f[4]):
            app_pss += int(f[6])
            new.append([f[0], f[3], f[4], int(f[6]), "app"])
        elif f[0] not in pids0:
            other_pss += int(f[6])
            new.append([f[0], f[3], f[4], int(f[6]), "other"])
    res.update({"app_pss_kib": app_pss, "other_new_pss_kib": other_pss, "new_processes": new})
    if save_png:
        vm.png(f"app-{key}")
    kill_app(a, unit, argv)
    return res


def apps(vm, out, variant, warm=5, save_png=False):
    a = vm.agent
    results = {}
    if variant == "gnome":
        vm.key("esc")
        time.sleep(2)
    order = DESKTOPS[variant]["apps"]
    for key, title, argv in order:
        log("cold", title)
        results[key] = {"title": title, "argv": argv, "runs": [launch(vm, out, variant, key, argv, 0, save_png)]}
    for n in range(1, warm + 1):
        for key, title, argv in order:
            log("warm", n, title)
            results[key]["runs"].append(launch(vm, out, variant, key, argv, n))
    return results


# ---------------------------------------------------------------- setup


def write_pdf(path):
    """A 12-page text PDF in the standard Helvetica font (no embedded fonts)."""
    pages = 12
    objs = ["<< /Type /Catalog /Pages 2 0 R >>",
            "<< /Type /Pages /Kids [" + " ".join(f"{3 + 2 * i} 0 R" for i in range(pages)) + f"] /Count {pages} >>"]
    lorem = ("Sekka benchmark fixture. The quick brown fox jumps over the lazy dog. "
             "Pack my box with five dozen liquor jugs. 0123456789")
    for i in range(pages):
        lines = [f"BT /F1 20 Tf 72 760 Td (Page {i + 1} of {pages}) Tj ET"]
        for j in range(40):
            lines.append(f"BT /F1 10 Tf 72 {730 - j * 16} Td ({lorem} {j}) Tj ET")
        stream = "\n".join(lines)
        objs.append(f"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents {4 + 2 * i} 0 R "
                    f"/Resources << /Font << /F1 << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> >> >> >>")
        objs.append(f"<< /Length {len(stream)} >>\nstream\n{stream}\nendstream")
    out = "%PDF-1.4\n"
    offsets = []
    for n, o in enumerate(objs, 1):
        offsets.append(len(out))
        out += f"{n} 0 obj\n{o}\nendobj\n"
    xref = len(out)
    out += f"xref\n0 {len(objs) + 1}\n0000000000 65535 f \n" + "".join(f"{o:010d} 00000 n \n" for o in offsets)
    out += f"trailer\n<< /Size {len(objs) + 1} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n"
    Path(path).write_text(out)


def put_file(agent, local, remote, owner="demo"):
    data = base64.b64encode(Path(local).read_bytes()).decode()
    agent.ok(f"mkdir -p {shlex.quote(str(Path(remote).parent))} && printf %s {data} | base64 -d > {shlex.quote(remote)} "
             f"&& chown -R {owner}: {shlex.quote(str(Path(remote).parent))}")


def dismiss_first_run(vm, variant, out):
    a = vm.agent
    notes = {}
    notes["processes"] = a.run("ps -u demo -o pid,comm,args --no-headers | cut -c1-200")[1]
    if variant == "gnome":
        # GNOME's first-login setup (existing-user mode) and the Shell's
        # welcome tour offer: mark both as seen, as finishing them would.
        a.user("mkdir -p ~/.config && echo yes > ~/.config/gnome-initial-setup-done")
        ver = a.run("gnome-shell --version | awk '{print $3}'")[1].strip()
        a.user(f"gsettings set org.gnome.shell welcome-dialog-last-shown-version '{ver}'", check=False)
        a.run("pkill -u demo -f gnome-initial-setup; pkill -u demo -f gnome-tour")
    elif variant == "plasma":
        a.run("pkill -u demo -f plasma-welcome")
    elif variant == "aizome":
        a.run("pkill -u demo -f 'aizome-annai'")
    time.sleep(3)
    vm.png("first-login-dismissed")
    return notes


def setup(args):
    variant = args.variant
    paths = nix_paths(variant)
    store = store_image(variant, paths)
    img = ROOT / "img" / variant
    out = ROOT / "setup" / variant
    shutil.rmtree(out, ignore_errors=True)
    out.mkdir(parents=True)
    golden = img / "golden.qcow2"
    if golden.exists():
        golden.unlink()
    new_root(golden)
    swap = img / "swap-setup.img"
    new_swap(swap, 4096)
    write_pdf(out / "fixture.pdf")
    record = {"paths": paths}
    spec = DESKTOPS[variant]

    # Boot 1: the first boot and first login, on the golden image itself.
    vm = VM(variant, out / "boot1", golden, store, swap, paths["toplevel"])
    try:
        t0 = vm.start()
        a = vm.connect_agent(400)
        log("agent up after", round(time.monotonic() - t0, 1), "s")
        if spec["greeter"]:
            ok = wait_guest(a, spec["greeter"], 120)
            log("greeter process", ok)
            w = watch(vm, t0, quiet=5.0, timeout=120, need_change=True)
            vm.png("first-greeter")
            record["first_greeter"] = {k: w.get(k) for k in ("settled", "timeout")}
            record["greeter_processes"] = a.run("ps -eo user:16,pid,comm,args --no-headers | grep -v '\\[' | cut -c1-200")[1]
            tl = login(vm, variant)
            ok = wait_guest(a, spec["session"], 120)
            log("session process", ok)
            if not ok:
                vm.png("login-failed")
                raise RuntimeError("no session after login")
            w = watch(vm, tl, quiet=8.0, timeout=90, need_change=True)
            record["first_login"] = {k: w.get(k) for k in ("settled", "timeout")}
            vm.png("first-login")
            time.sleep(10)
            vm.png("first-login-later")
            record["first_run"] = dismiss_first_run(vm, variant, out)
            put_file(a, out / "fixture.pdf", PDF)
            # Open every app once, so later first launches after boot find
            # the caches a used system has.
            if variant == "gnome":
                vm.key("esc")
                time.sleep(2)
            record["warmup"] = {}
            for key, title, argv in spec["apps"]:
                r = launch(vm, vm.dir, variant, key, argv, 0, save_png=True)
                r.pop("frames")
                record["warmup"][key] = r
                log("warm-up", title, r["settled"], r["timeout"])
            r = probe(vm, vm.dir, "probe-check", "latency", 20, (20, 120, 250))
            record["probe_check"] = r
            log("probe check", r)
            record["session_env"] = a.run("systemctl --user -M demo@ show-environment")[1]
        else:
            w = watch(vm, t0, quiet=5.0, timeout=120, need_change=True)
            vm.png("first-console")
        journal(vm, vm.dir)
    finally:
        vm.shutdown()
        vm.kill()
    (out / "setup.json").write_text(json.dumps(record, indent=1, default=str))

    # Boot 2, on a throwaway overlay: the reference frames trials match.
    refs = img / "refs"
    refs.mkdir(exist_ok=True)
    ov = img / "setup-overlay.qcow2"
    if ov.exists():
        ov.unlink()
    overlay(golden, ov)
    new_swap(swap, 4096)
    vm = VM(variant, out / "boot2", ov, store, swap, paths["toplevel"])
    try:
        t0 = vm.start()
        a = vm.connect_agent(400)
        if spec["greeter"]:
            wait_guest(a, spec["greeter"], 120)
            w = watch(vm, t0, quiet=8.0, timeout=180, need_change=True)
            save_ppm(w["final"], refs / "greeter.ppm")
            vm.png("ref-greeter")
            tl = login(vm, variant)
            wait_guest(a, spec["session"], 120)
            w = watch(vm, tl, quiet=10.0, timeout=180, need_change=True)
            save_ppm(w["final"], refs / "desktop.ppm")
            vm.png("ref-desktop")
            record["second_login"] = {k: w.get(k) for k in ("settled", "timeout")}
            record["second_login_processes"] = a.run("ps -u demo -o pid,comm --no-headers")[1]
        else:
            w = watch(vm, t0, quiet=8.0, timeout=180, need_change=True)
            save_ppm(w["final"], refs / "greeter.ppm")
            vm.png("ref-console")
    finally:
        vm.shutdown()
        vm.kill()
        ov.unlink(missing_ok=True)
    (out / "setup.json").write_text(json.dumps(record, indent=1, default=str))
    log("setup done", out)


# ---------------------------------------------------------------- trial


def trial_clock(golden):
    """Every measured boot starts at the same time of day, 15:00 UTC, on the
    day the golden disk was made (or the next, if it was made later than
    that): Aizome's wallpaper and theme follow day and night, and the
    reference frames were captured by day."""
    import datetime
    made = datetime.datetime.fromtimestamp(golden.stat().st_mtime, datetime.timezone.utc)
    at = made.replace(hour=15, minute=0, second=0, microsecond=0)
    if at <= made:
        at += datetime.timedelta(days=1)
    return at.strftime("%Y-%m-%dT%H:%M:%S")


def trial(args):
    variant = args.variant
    spec = DESKTOPS[variant]
    paths = nix_paths(variant)
    store = store_image(variant, paths)
    img = ROOT / "img" / variant
    golden = img / "golden.qcow2"
    refs = img / "refs"
    out = ROOT / "runs" / f"{variant}-{args.mem}" / args.tag
    shutil.rmtree(out, ignore_errors=True)
    out.mkdir(parents=True)
    root = out / "root.qcow2"
    overlay(golden, root)
    swap = ROOT / "img" / variant / f"swap-{args.tag}.img"
    new_swap(swap, args.mem)
    result = {"variant": variant, "tag": args.tag, "mem_mib": args.mem, "cpus": CPUS, "paths": paths,
              "host": {"start": time.strftime("%Y-%m-%dT%H:%M:%S%z"), "uname": os.uname().release}}
    result["host"]["quiet_wait"] = wait_quiet_host()
    phases = set(args.phases.split(","))
    if "hostidle" in phases:
        p = Power(out / "host-idle-power.json")
        time.sleep(args.host_idle)
        result["host_idle_power"] = p.stop()
        result["host"]["loadavg_after_hostidle"] = loadavg()
    greeter_ref = load_ppm(refs / "greeter.ppm")
    desktop_ref = load_ppm(refs / "desktop.ppm") if (refs / "desktop.ppm").exists() else None
    rtc = trial_clock(golden)
    result["guest_rtc_start"] = rtc
    vm = VM(variant, out, root, store, swap, paths["toplevel"], mem=args.mem, rtc=rtc)
    frames = {}
    try:
        t0 = vm.start()
        log(variant, args.tag, "booting")
        # Boot: QEMU started -> login screen (or console) visible and settled.
        # The console has no clock: match it exactly (a booting console is
        # also mostly black).
        tol = 0.001 if variant == "baseline" else 0.02
        w = watch(vm, t0, ref=greeter_ref, ref_tol=tol, quiet=3.0, timeout=300, interval=0.1)
        frames["boot"] = w.pop("frames")
        result["boot"] = {k: v for k, v in w.items() if k != "final"}
        log("boot settled", w.get("settled"), "timeout" if w["timeout"] else "")
        if args.png:
            vm.png("greeter")
        t_greeter = t0 + (w.get("settled") or 0)
        # Quiet at the login screen, then look inside.
        time.sleep(max(0, t_greeter + 30 - time.monotonic()))
        a = vm.connect_agent(120)
        result["clock"] = vm.guest_clock_offset()
        result["clock"]["host_t0"] = t0
        memory(vm, out, "greeter")
        result["systemd_analyze"] = analyze(vm, out, variant)
        result["loadavg_greeter"] = loadavg()
        if spec["greeter"]:
            time.sleep(3)
            _, before = vm.grab()
            tl = login(vm, variant)
            w = watch(vm, tl, ref=desktop_ref, base=before, min_from_base=20000, quiet=3.0, timeout=300, interval=0.1)
            frames["login"] = w.pop("frames")
            result["login"] = {k: v for k, v in w.items() if k != "final"}
            result["login"]["host_enter"] = tl
            log("login settled", w.get("settled"), "timeout" if w["timeout"] else "")
            if args.png:
                vm.png("desktop")
            if variant == "gnome":
                # GNOME opens its overview at login. Close it, so the idle
                # and memory figures are for the empty desktop, as the
                # others show it.
                time.sleep(1)
                vm.key("esc")
        else:
            tl = time.monotonic() + 10
        # Memory and idle cost at the desktop.
        if "idle" in phases:
            time.sleep(max(0, tl + 60 - time.monotonic()))
            memory(vm, out, "desktop60")
            time.sleep(max(0, tl + 65 - time.monotonic()))
            result["idle"] = idle_window(vm, out, 120)
            result["idle"]["start_after_login_s"] = 65
            time.sleep(max(0, tl + 190 - time.monotonic()))
            memory(vm, out, "desktop190")
        result["loadavg_desktop"] = loadavg()
        journal(vm, out)
        if spec["greeter"] and "latency" in phases:
            log("responsiveness")
            result["responsiveness"] = responsiveness(vm, out, variant, args.presses)
        if spec["greeter"] and "apps" in phases:
            log("apps")
            result["apps"] = apps(vm, out, variant, warm=args.warm, save_png=args.png)
            if args.png:
                vm.png("after-apps")
        result["host"]["end"] = time.strftime("%Y-%m-%dT%H:%M:%S%z")
        result["host"]["loadavg_end"] = loadavg()
        result["serial_tail"] = (out / "serial.log").read_text(errors="replace")[-4000:]
    finally:
        try:
            vm.shutdown()
        finally:
            vm.kill()
            root.unlink(missing_ok=True)
            swap.unlink(missing_ok=True)
        with lzma.open(out / "frames.json.xz", "wt") as f:
            json.dump(frames, f)
        (out / "result.json").write_text(json.dumps(result, indent=1, default=str))
    log("trial done", out)


def shell(args):
    variant = args.variant
    paths = nix_paths(variant)
    store = store_image(variant, paths)
    img = ROOT / "img" / variant
    out = ROOT / "shell" / variant
    shutil.rmtree(out, ignore_errors=True)
    out.mkdir(parents=True)
    root = out / "root.qcow2"
    overlay(img / "golden.qcow2", root)
    swap = out / "swap.img"
    new_swap(swap, args.mem)
    vm = VM(variant, out, root, store, swap, paths["toplevel"], mem=args.mem)
    vm.start()
    log("pid", vm.pid(), "sockets", vm.sockdir)
    try:
        vm.proc.wait()
    except KeyboardInterrupt:
        vm.kill()


def main():
    ap = argparse.ArgumentParser()
    sub = ap.add_subparsers(dest="cmd", required=True)
    s = sub.add_parser("setup")
    s.add_argument("variant")
    t = sub.add_parser("trial")
    t.add_argument("variant")
    t.add_argument("tag")
    t.add_argument("--mem", type=int, default=4096)
    t.add_argument("--phases", default="hostidle,idle,latency,apps")
    t.add_argument("--host-idle", type=float, default=30)
    t.add_argument("--warm", type=int, default=5)
    t.add_argument("--png", action="store_true")
    t.add_argument("--presses", type=int, default=220)
    sh = sub.add_parser("shell")
    sh.add_argument("variant")
    sh.add_argument("--mem", type=int, default=4096)
    args = ap.parse_args()
    {"setup": setup, "trial": trial, "shell": shell}[args.cmd](args)


if __name__ == "__main__":
    main()
