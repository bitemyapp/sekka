#!/usr/bin/env python3
"""One terminal in a private headless sway: startup, key-to-frame latency,
paced streams, throughput, energy, memory and idle wakeups.

Extended from apps/hotaru/tools/frame-benchmark.py (Hotaru against Ghostty)
with adapters for gnome-terminal, Konsole, GNOME Console (kgx), foot and a few
others, a default-configuration mode, process-group accounting (a terminal's
server process counts as the terminal), PSS, context switches, DRM client
statistics and the AMD socket power sensor (PPT).

Each timestamp ends at the observer's receipt of a matching screencopy of the
private compositor's output: not physical scanout. Markers carry a sequence
number and CRC; they prove which update a frame shows, not every glyph.
"""
import argparse
import contextlib
import datetime
import fcntl
import hashlib
import json
import os
from pathlib import Path
import queue
import random
import select
import shutil
import signal
import socket
import struct
import subprocess
import sys
import tempfile
import threading
import time
import tty
import zlib
from xml.sax.saxutils import escape

GUARD = (0xF71F35, 0x1FD7EF, 0xE7BD13, 0x2941E9, 0xF3F3F3, 0x111111)
BITS = (0x20E050, 0xC028E0)
MESA_EGL = "/run/opengl-driver/share/glvnd/egl_vendor.d/50_mesa.json"
RADEON_VK = "/run/opengl-driver/share/vulkan/icd.d/radeon_icd.x86_64.json"
GNOME_TERMINAL_PROFILE = "b1dcc9dd-5262-4d8d-a863-c897e6d979b9"  # the schema's built-in default profile


def cpu_set(value):
    result = set()
    for part in value.split(","):
        edges = part.split("-")
        first, last = int(edges[0]), int(edges[-1])
        if len(edges) > 2 or first < 0 or last < first or last > 4095:
            raise ValueError("invalid CPU range")
        result.update(range(first, last + 1))
    return result


def send(stream, value):
    stream.write(json.dumps(value).encode() + b"\n")
    stream.flush()


def receive(stream):
    line = stream.readline(1024 * 1024)
    if not line.endswith(b"\n"):
        raise RuntimeError("missing/oversized fixture message")
    return json.loads(line)


def crc(sequence, nonce):
    return zlib.crc32(struct.pack(">II", sequence, nonce))


def marker(sequence, nonce, row):
    bits = sequence << 32 | crc(sequence, nonce)
    colors = (*GUARD, *(BITS[(bits >> at) & 1] for at in range(63, -1, -1)), *reversed(GUARD))
    return (f"\x1b[{row};1H" + "".join(
        f"\x1b[48;2;{color >> 16};{color >> 8 & 255};{color & 255}m " for color in colors) + "\x1b[0m").encode()


def output_bytes(sequence, nonce, cols, rows, kind):
    if cols < 76 or rows < 4:
        raise RuntimeError(f"marker needs at least 76x4 cells, received {cols}x{rows}")
    data = bytearray(b"\x1b[?25l\x1b[?7l\x1b[0m")
    if kind == "initial":
        data.extend(b"\x1b]10;rgb:a9/b1/d6\x1b\\\x1b]11;rgb:15/16/1e\x1b\\\x1b[r\x1b[2J")
    data.extend(marker(sequence, nonce, 1))
    if kind == "initial":
        for row in range(2, rows):
            text = (f"row {row:03} | abcdefgh 0123456789 != -> === " * (cols // 32 + 1))[:cols]
            data.extend(f"\x1b[{row};1H{text}".encode())
    elif kind in ("sparse", "keyboard"):
        row, col = 2 + sequence % (rows - 2), 1 + sequence * 7 % cols
        data.extend(f"\x1b[{row};{col}H{chr(65 + sequence % 26)}".encode())
    elif kind == "scroll":
        text = (f"line {sequence:08} | abcdefgh 0123456789 != -> === " * (cols // 32 + 1))[:cols]
        data.extend(f"\x1b[2;{rows - 1}r\x1b[{rows - 1};1H\n\r{text}".encode())
    elif kind == "marker":
        pass  # Dynamic marker-cost control, separate from static capture cost.
    else:
        raise RuntimeError(f"unknown workload {kind}")
    data.extend(marker(sequence, nonce, rows))
    return bytes(data)


# What an idle shell shows: terminal defaults back (autowrap, visible cursor,
# the theme's own colors), a cleared screen and a prompt. The cursor's
# blinking stays whatever the terminal's configuration makes it.
PROMPT = b"\x1b[?7h\x1b[?25h\x1b[0m\x1b[r\x1b]110\x1b\\\x1b]111\x1b\\\x1b[2J\x1b[H$ "


def write_output(data):
    before = time.monotonic_ns()
    offset = 0
    while offset < len(data):
        offset += os.write(1, data[offset:offset + 65536])
    return before, time.monotonic_ns()


def fixture(address, nonce):
    if os.environ.get("HOTARU_BENCH_FIXTURE_CPUS"):
        os.sched_setaffinity(0, cpu_set(os.environ["HOTARU_BENCH_FIXTURE_CPUS"]))
    tty.setraw(0)
    resize_read, resize_write = os.pipe2(os.O_CLOEXEC | os.O_NONBLOCK)
    signal.signal(signal.SIGWINCH, lambda *_: None)
    signal.set_wakeup_fd(resize_write)
    with socket.socket(socket.AF_UNIX) as connection:
        connection.connect(address)
        stream = connection.makefile("rwb", buffering=0)
        cols, rows = os.get_terminal_size(0)
        send(stream, {"event": "ready", "ready_ns": time.monotonic_ns(), "pid": os.getpid(), "cols": cols, "rows": rows,
                      "cpu_affinity": sorted(os.sched_getaffinity(0)), "term": os.environ.get("TERM")})
        armed = []
        echo = {"on": False, "column": 2}

        def paint(sequence, kind, prepared=None, read_ns=None):
            cols, rows = os.get_terminal_size(0)
            data = prepared if prepared is not None else output_bytes(sequence, nonce, cols, rows, kind)
            before, after = write_output(data)
            send(stream, {"event": "write", "sequence": sequence, "kind": kind, "crc": crc(sequence, nonce),
                          "read_ns": read_ns, "write_start_ns": before, "write_end_ns": after,
                          "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest(), "cols": cols, "rows": rows})

        startup = True
        if cols >= 76 and rows >= 4:
            paint(0, "initial")
        while True:
            readable, _, _ = select.select([0, connection, resize_read], [], [])
            if resize_read in readable:
                os.read(resize_read, 4096)
                cols, rows = os.get_terminal_size(0)
                if startup and cols >= 76 and rows >= 4:
                    paint(0, "initial")
            if connection in readable:
                request = receive(stream)
                operation = request["operation"]
                if operation == "size":
                    cols, rows = os.get_terminal_size(0)
                    send(stream, {"event": "size", "cols": cols, "rows": rows})
                elif operation == "paint":
                    startup = False
                    paint(request["sequence"], request["kind"])
                elif operation == "arm":
                    cols, rows = os.get_terminal_size(0)
                    armed = [(seq, output_bytes(seq, nonce, cols, rows, "keyboard"))
                             for seq in range(request["start"], request["start"] + request["count"])]
                    send(stream, {"event": "armed", "count": len(armed)})
                elif operation == "prompt":
                    startup = False
                    before, after = write_output(PROMPT)
                    echo.update(on=request.get("echo", False), column=2)
                    send(stream, {"event": "prompt", "write_start_ns": before, "write_end_ns": after, "echo": echo["on"]})
                elif operation == "stream":
                    cols, rows = os.get_terminal_size(0)
                    prepared = [(seq, output_bytes(seq, nonce, cols, rows, request["kind"]))
                                for seq in range(request["start"], request["start"] + request["count"])]
                    send(stream, {"event": "stream_ready", "count": len(prepared)})
                    start = time.monotonic_ns()
                    for index, (seq, data) in enumerate(prepared):
                        if request["hz"]:
                            deadline = start + int(index * 1e9 / request["hz"])
                            if deadline > time.monotonic_ns():
                                time.sleep((deadline - time.monotonic_ns()) / 1e9)
                        paint(seq, request["kind"], data)
                    send(stream, {"event": "stream_done", "time_ns": time.monotonic_ns()})
                elif operation == "throughput":
                    # The PTY takes only what the terminal consumes, so the
                    # write time is the terminal's own speed. Terminal defaults
                    # first (autowrap, cursor), as a fresh terminal would see
                    # the input; whole repeats, so the closing marker can't land
                    # inside an unfinished sequence.
                    data = Path(request["path"]).read_bytes()
                    write_output(b"\x1b[?7h\x1b[?25h\x1b[0m\x1b[r\x1b[2J\x1b[H")
                    start = time.monotonic_ns()
                    for _ in range(request["repeat"]):
                        write_output(data)
                    end = time.monotonic_ns()
                    send(stream, {"event": "throughput_done", "path": request["path"], "bytes": len(data) * request["repeat"],
                                  "write_start_ns": start, "write_end_ns": end})
                    paint(request["sequence"], "initial")
                elif operation == "quit":
                    return
                else:
                    raise RuntimeError(f"unknown fixture operation {operation}")
            if 0 in readable:
                data = os.read(0, 4096)
                read_ns = time.monotonic_ns()
                if not data:
                    return
                if echo["on"]:
                    # A shell's echo of typed text, wrapping to a new prompt.
                    out = bytearray()
                    for byte in data:
                        out.append(byte)
                        echo["column"] += 1
                        if echo["column"] >= 100:
                            out.extend(b"\r\n$ ")
                            echo["column"] = 2
                    write_output(bytes(out))
                    continue
                for byte in data:
                    if byte != ord("a") or not armed:
                        raise RuntimeError(f"unexpected PTY input {data!r} with {len(armed)} trials armed")
                    seq, prepared = armed.pop(0)
                    paint(seq, "keyboard", prepared, read_ns)


def wait(label, predicate, timeout=30):
    end = time.monotonic() + timeout
    while time.monotonic() < end:
        value = predicate()
        if value:
            return value
        time.sleep(.01)
    raise RuntimeError(f"timed out: {label}")


class JsonReader:
    def __init__(self, stream, path):
        self.events = []
        self.queue = queue.Queue()
        self.failure = None
        self.file = path.open("w")
        self.thread = threading.Thread(target=self._read, args=(stream,), daemon=True)
        self.thread.start()

    def _read(self, stream):
        try:
            while True:
                value = receive(stream)
                value["controller_received_ns"] = time.monotonic_ns()
                self.events.append(value)
                self.file.write(json.dumps(value, separators=(",", ":")) + "\n")
                self.file.flush()
                self.queue.put(value)
        except Exception as error:
            self.failure = error
            self.queue.put(None)

    def until(self, predicate, timeout=30):
        end = time.monotonic() + timeout
        while time.monotonic() < end:
            try:
                event = self.queue.get(timeout=max(.001, end - time.monotonic()))
            except queue.Empty as error:
                raise RuntimeError("timed out waiting for event") from error
            if event is None:
                raise RuntimeError(f"reader ended: {self.failure}")
            if predicate(event):
                return event
        raise RuntimeError("event timeout")

    def drain(self):
        with contextlib.suppress(queue.Empty):
            while True:
                self.queue.get_nowait()


def source_hash(path):
    with open(path, "rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def build_observer(directory, xml):
    """Compile the screencopy/virtual-keyboard observer from protocol XML files."""
    directory.mkdir(parents=True, exist_ok=True)
    protocols, sources = {}, []
    for name, path in xml.items():
        protocols[name] = {"path": str(path), "sha256": source_hash(path)}
        header, code = directory / f"{name}-client.h", directory / f"{name}-protocol.c"
        subprocess.run(["wayland-scanner", "client-header", str(path), str(header)], check=True)
        subprocess.run(["wayland-scanner", "private-code", str(path), str(code)], check=True)
        sources.append(str(code))
    flags = subprocess.check_output(["pkg-config", "--cflags", "--libs", "wayland-client", "xkbcommon"], text=True).split()
    source = Path(__file__).with_name("frame-observer.c")
    binary = directory / "frame-observer"
    command = ["cc", "-O2", "-std=c11", "-Wall", "-Wextra", "-Werror", "-I" + str(directory), str(source), *sources, *flags, "-o", str(binary)]
    subprocess.run(command, check=True)
    sampler = directory / "ppt-sampler"
    subprocess.run(["cc", "-O2", "-std=c11", "-Wall", "-Wextra", "-Werror", str(Path(__file__).with_name("ppt-sampler.c")),
                    "-o", str(sampler)], check=True)
    metadata = {"command": command, "source_sha256": source_hash(source), "binary_sha256": source_hash(binary),
                "sampler_sha256": source_hash(sampler), "protocols": protocols,
                "compiler": subprocess.check_output(["cc", "--version"], text=True).splitlines()[0],
                "pkg_config_versions": subprocess.check_output(["pkg-config", "--modversion", "wayland-client", "xkbcommon"], text=True).splitlines()}
    (directory / "build.json").write_text(json.dumps(metadata, indent=2) + "\n")
    return binary, sampler


def nodes(node):
    yield node
    for child in node.get("nodes", []) + node.get("floating_nodes", []):
        yield from nodes(child)


def percentile(values, fraction):
    if not values:
        return None
    return sorted(values)[min(len(values) - 1, int((len(values) - 1) * fraction))]


def parents():
    """Parent of every live process (zombies, which hold nothing, left out)."""
    table = {}
    for proc in Path("/proc").iterdir():
        if proc.name.isdecimal():
            with contextlib.suppress(OSError, ValueError, IndexError):
                fields = (proc / "stat").read_text().rsplit(")", 1)[1].split()
                if fields[0] != "Z":
                    table[int(proc.name)] = int(fields[1])
    return table


def descendants(roots, table=None):
    """The roots that still exist and every process descended from them."""
    table = parents() if table is None else table
    tree = [r for r in roots if r in table]
    frontier = list(tree)
    while frontier:
        frontier = [pid for pid, parent in table.items() if parent in frontier]
        tree.extend(frontier)
    return tree


def read(path):
    with contextlib.suppress(OSError):
        return Path(path).read_text()
    return None


def proc_stats(pid):
    """CPU (ticks from stat, which keep exited threads' time; and per-thread
    schedstat nanoseconds), faults and per-thread context switches."""
    stat = read(f"/proc/{pid}/stat")
    if stat is None:
        return None
    comm = stat[stat.index("(") + 1:stat.rindex(")")]
    fields = stat.rsplit(")", 1)[1].split()
    tasks = {}
    with contextlib.suppress(OSError):
        for task in Path(f"/proc/{pid}/task").iterdir():
            schedstat, status = read(task / "schedstat"), read(task / "status")
            if schedstat is None or status is None:
                continue
            switches = {line.split(":")[0]: int(line.split()[1]) for line in status.splitlines()
                        if line.startswith(("voluntary_ctxt_switches", "nonvoluntary_ctxt_switches"))}
            tasks[task.name] = [int(schedstat.split()[0]), switches.get("voluntary_ctxt_switches", 0),
                                switches.get("nonvoluntary_ctxt_switches", 0)]
    return {"comm": comm, "ticks": int(fields[11]) + int(fields[12]), "faults_minor": int(fields[7]),
            "faults_major": int(fields[9]), "threads": int(fields[17]), "tasks": tasks}


def group_sample(pids):
    sample = {"t": time.monotonic_ns(), "procs": {}}
    for pid in pids:
        stats = proc_stats(pid)
        if stats is not None:
            sample["procs"][str(pid)] = stats
    return sample


def group_delta(before, after, ticks_per_second):
    """CPU and context switches between two samples of a process group.
    A process or thread that appears in `after` only started inside the
    interval and counts whole; one that disappeared is lost and counted."""
    result = {"seconds": (after["t"] - before["t"]) / 1e9, "cpu_ms_ticks": 0.0, "cpu_ms_sched": 0.0,
              "ctxt_voluntary": 0, "ctxt_involuntary": 0, "processes": len(after["procs"]),
              "threads": sum(len(p["tasks"]) for p in after["procs"].values()), "vanished": 0, "faults_minor": 0}
    for pid, new in after["procs"].items():
        old = before["procs"].get(pid)
        result["cpu_ms_ticks"] += (new["ticks"] - (old["ticks"] if old else 0)) * 1000 / ticks_per_second
        result["faults_minor"] += new["faults_minor"] - (old["faults_minor"] if old else 0)
        for tid, (run, vol, invol) in new["tasks"].items():
            prior = old["tasks"].get(tid) if old else None
            run0, vol0, invol0 = prior if prior else (0, 0, 0)
            result["cpu_ms_sched"] += (run - run0) / 1e6
            result["ctxt_voluntary"] += vol - vol0
            result["ctxt_involuntary"] += invol - invol0
    result["vanished"] = sum(1 for pid in before["procs"] if pid not in after["procs"])
    seconds = result["seconds"] or 1
    result["ctxt_per_second"] = (result["ctxt_voluntary"] + result["ctxt_involuntary"]) / seconds
    result["cpu_percent"] = result["cpu_ms_sched"] / 10 / seconds
    return result


SMAPS_FIELDS = ("Rss", "Pss", "Pss_Dirty", "Pss_Anon", "Pss_File", "Pss_Shmem", "Shared_Clean", "Shared_Dirty",
                "Private_Clean", "Private_Dirty", "Swap", "SwapPss")


def drm_clients(pids):
    """amdgpu/DRM fdinfo for each DRM client the processes hold, deduplicated
    by client id: GPU engine time and the memory the driver charges them."""
    clients = {}
    for pid in pids:
        with contextlib.suppress(OSError):
            for fd in Path(f"/proc/{pid}/fd").iterdir():
                with contextlib.suppress(OSError):
                    target = os.readlink(fd)
                    if not target.startswith("/dev/dri/"):
                        continue
                    info = {}
                    for line in Path(f"/proc/{pid}/fdinfo/{fd.name}").read_text().splitlines():
                        if line.startswith(("drm-", "amd-")) and ":" in line:
                            key, value = line.split(":", 1)
                            info[key] = value.strip()
                    if "drm-client-id" in info:
                        clients.setdefault(f"{info.get('drm-pdev', '?')}/{info['drm-client-id']}",
                                           {"pid": pid, "device": target, **info})
    return clients


def drm_ns(clients, engine="drm-engine-gfx"):
    return sum(int(c[engine].split()[0]) for c in clients.values() if engine in c)


def memory_sample(pids):
    total = {name: 0 for name in SMAPS_FIELDS}
    processes = {}
    for pid in pids:
        rollup, status = read(f"/proc/{pid}/smaps_rollup"), read(f"/proc/{pid}/status")
        if rollup is None or status is None:
            continue
        values = {}
        for line in rollup.splitlines()[1:]:
            name = line.split(":", 1)[0]
            if name in SMAPS_FIELDS:
                values[name] = int(line.split()[1])
        for line in status.splitlines():
            if line.split(":", 1)[0] in ("VmRSS", "VmHWM", "Threads"):
                values[line.split(":", 1)[0]] = int(line.split()[1])
        values["comm"] = (read(f"/proc/{pid}/comm") or "").strip()
        with contextlib.suppress(OSError):
            values["exe"] = os.readlink(f"/proc/{pid}/exe")
        processes[str(pid)] = values
        for name in SMAPS_FIELDS:
            total[name] += values.get(name, 0)
    clients = drm_clients(pids)
    gpu = {}
    for client in clients.values():
        for key, value in client.items():
            if key.startswith(("drm-memory-", "drm-total-", "drm-resident-")) and value.endswith(("KiB", "MiB", "kB")):
                number, unit = value.split()
                gpu[key] = gpu.get(key, 0) + int(number) * (1024 if unit == "MiB" else 1)
    return {"t": time.monotonic_ns(), "total_kib": total, "drm_kib": gpu, "processes": processes, "drm_clients": clients}


class Power:
    """The PPT sampler: a background process, energy integrated afterwards."""

    def __init__(self, binary, path, cpus):
        self.path = path
        self.process = subprocess.Popen(["taskset", "-c", cpus, str(binary), "10", str(path)],
                                        stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, start_new_session=True)
        self.samples = None

    def stop(self):
        if self.process.poll() is None:
            self.process.send_signal(signal.SIGTERM)
            self.process.wait(timeout=5)
        samples = []
        for line in self.path.read_text().splitlines():
            if line and not line.startswith("#"):
                t, value = line.split("\t")
                samples.append((int(t), int(value) / 1000))  # milliwatts (see ppt-sampler.c) to watts
        self.samples = samples

    def window(self, name, start, end, **extra):
        points = [(t, w) for t, w in self.samples if start <= t <= end]
        before = [(t, w) for t, w in self.samples if t < start][-1:]
        after = [(t, w) for t, w in self.samples if t > end][:1]

        def at(t, a, b):
            return a[1] + (b[1] - a[1]) * (t - a[0]) / (b[0] - a[0]) if b[0] != a[0] else a[1]

        if before and (points or after):
            points.insert(0, (start, at(start, before[0], (points or after)[0])))
        if after and points:
            points.append((end, at(end, points[-1], after[0])))
        energy = sum((b[0] - a[0]) / 1e9 * (a[1] + b[1]) / 2 for a, b in zip(points, points[1:]))
        seconds = (end - start) / 1e9
        return {"name": name, "start_ns": start, "end_ns": end, "seconds": seconds, "energy_j": energy,
                "mean_w": energy / seconds if seconds > 0 else None, "samples": len(points), **extra}


# Adapters: how each terminal is configured and started. `matched` is the
# harness configuration (IBM Plex Mono 14 pt, cursor blink off, zero
# scrollback, opaque); `default` is the package's own defaults in an empty
# home, as a person first meets it. `--scrollback ghostty-default` keeps the
# matched configuration but gives every terminal history: Ghostty its own
# defaults (50 MB, unlimited lines), Hotaru its 50 MB byte limit with its row
# limit at the most it allows (1,000,000), and the others, whose history is
# counted in lines only, the same 1,000,000 lines (HISTORY_LINES). Zero
# scrollback turns off much of libghostty-vt's fast path for output on the
# primary screen, so throughput there should be measured with history.

HISTORY_LINES = 1_000_000


def history_lines(args):
    """Lines of history in the matched configuration."""
    return 0 if args.scrollback == "zero" else HISTORY_LINES


class Adapter:
    gtk4 = False
    services = ()          # D-Bus services this terminal may activate on the private bus
    dconf = False          # reads settings through GSettings/dconf

    def __init__(self, binary, args):
        self.binary, self.args = binary, args

    def env(self, runtime):
        return {}

    def dconf_keyfile(self, mode):
        return None


class Hotaru(Adapter):
    app_id = "org.aizome.Hotaru"

    def setup(self, runtime, mode, child):
        if mode == "default":
            return {}, [str(self.binary), "--", *child]
        # Hotaru's history keeps at most `scrollback` rows and 50 MB
        # (hotaru-core's DEFAULT_HISTORY_BYTES), whichever comes first.
        rows = history_lines(self.args)
        text = (f'font_size = {self.args.font_size}\nscrollback = {rows}\ntheme = "night"\nshell_integration = false\n'
                'cursor_blink = false\n')
        (runtime / "hotaru.toml").write_text(text)
        return {"hotaru.toml": text}, [str(self.binary), "--config-file", str(runtime / "hotaru.toml"), "--", *child]

    def env(self, runtime):
        return {"AIZOME_FONT_DIR": str(self.args.font_dir), "HOTARU_FONT_DIR": str(self.args.fallback_dir),
                "AIZOME_NO_PIXEL_SCROLL": "0", "RUST_LOG": "warn"}


class Ghostty(Adapter):
    app_id = "com.mitchellh.ghostty"
    gtk4 = True

    def setup(self, runtime, mode, child):
        if mode == "default":
            return {}, [str(self.binary), "-e", *child]
        # Without the two scrollback lines Ghostty keeps its defaults:
        # 50 MB (scrollback-limit-bytes) and unlimited lines.
        history = ("scrollback-limit-bytes = 0\nscrollback-limit-lines = 0\n" if self.args.scrollback == "zero" else "")
        text = ("gtk-single-instance = false\nshell-integration = none\n" + history +
                f"font-family = IBM Plex Mono\nfont-size = {self.args.font_size}\ncursor-style-blink = false\nbackground-opacity = 1\n"
                "adjust-cell-height = -1\n")
        (runtime / "ghostty.conf").write_text(text)
        return {"ghostty.conf": text}, [str(self.binary), "--config-default-files=false", "--config-file=" + str(runtime / "ghostty.conf"), "-e", *child]


class Foot(Adapter):
    app_id = "foot"

    def setup(self, runtime, mode, child):
        if mode == "default":
            return {}, [str(self.binary), "--", *child]
        # foot's line height from the font's rounded metrics is 26 px; its
        # supported override gives the 24 px rows Hotaru, Ghostty and Konsole draw.
        text = (f"font=IBM Plex Mono:size={self.args.font_size:g}\nline-height=24px\n\n[scrollback]\nlines={history_lines(self.args)}\n\n[cursor]\nblink=no\n\n"
                "[colors-dark]\nalpha=1.0\n")
        (runtime / "foot.ini").write_text(text)
        return {"foot.ini": text}, [str(self.binary), "--config", str(runtime / "foot.ini"), "--", *child]


class Konsole(Adapter):
    app_id = "org.kde.konsole"

    def setup(self, runtime, mode, child):
        if mode == "default":
            return {}, [str(self.binary), "-e", *child]
        profile = ("[General]\nName=Bench\nParent=FALLBACK/\n\n"
                   f"[Appearance]\nFont=IBM Plex Mono,{self.args.font_size:g},-1,5,400,0,0,0,0,0,0,0,0,0,0,1\n\n"
                   # HistoryMode 0 keeps none, 1 keeps HistorySize lines.
                   + ("[Scrolling]\nHistoryMode=0\n\n" if self.args.scrollback == "zero"
                      else f"[Scrolling]\nHistoryMode=1\nHistorySize={history_lines(self.args)}\n\n")
                   + "[Terminal Features]\nBlinkingCursorEnabled=false\n")
        rc = "[Desktop Entry]\nDefaultProfile=Bench.profile\n"
        (runtime / "data/konsole").mkdir(parents=True, exist_ok=True)
        (runtime / "data/konsole/Bench.profile").write_text(profile)
        (runtime / "config/konsolerc").write_text(rc)
        return {"data/konsole/Bench.profile": profile, "config/konsolerc": rc}, [str(self.binary), "-e", *child]

    def env(self, runtime):
        return {"QT_QPA_PLATFORM": "wayland"}


class GnomeTerminal(Adapter):
    """A client asks gnome-terminal-server, activated on the private bus, for
    a window; the server (VTE, GTK 3) is the terminal and is counted as it."""
    app_id = "org.gnome.Terminal"
    services = ("org.gnome.Terminal",)
    dconf = True

    def setup(self, runtime, mode, child):
        return {}, [str(self.binary / "bin/gnome-terminal"), "--", *child]

    def dconf_keyfile(self, mode):
        if mode == "default":
            return None
        return (f"[org/gnome/terminal/legacy/profiles:/:{GNOME_TERMINAL_PROFILE}]\n"
                f"font='IBM Plex Mono {self.args.font_size:g}'\nuse-system-font=false\ncursor-blink-mode='off'\n"
                f"scrollback-lines={history_lines(self.args)}\nscrollback-unlimited=false\nuse-transparent-background=false\naudible-bell=false\n")


class Kgx(Adapter):
    app_id = "org.gnome.Console"
    gtk4 = True
    dconf = True

    def setup(self, runtime, mode, child):
        return {}, [str(self.binary), "--", *child]

    def dconf_keyfile(self, mode):
        if mode == "default":
            return None
        # Console has no blink setting of its own: VTE follows the desktop's.
        return (f"[org/gnome/Console]\ncustom-font='IBM Plex Mono {self.args.font_size:g}'\nuse-system-font=false\n"
                f"scrollback-lines=int64 {history_lines(self.args)}\nignore-scrollback-limit=false\ntransparency=false\naudible-bell=false\n\n"
                "[org/gnome/desktop/interface]\ncursor-blink=false\n")


class Alacritty(Adapter):
    """Not measured for the write-up: an adapter only, untested."""
    app_id = "Alacritty"

    def setup(self, runtime, mode, child):
        if mode == "default":
            return {}, [str(self.binary), "-e", *child]
        text = (f'[font]\nsize = {self.args.font_size}\nnormal = {{ family = "IBM Plex Mono" }}\n\n'
                "[scrolling]\nhistory = 0\n\n[cursor.style]\nblinking = \"Never\"\n\n[window]\nopacity = 1.0\n")
        (runtime / "alacritty.toml").write_text(text)
        return {"alacritty.toml": text}, [str(self.binary), "--config-file", str(runtime / "alacritty.toml"), "-e", *child]


class Kitty(Adapter):
    """Not measured for the write-up: an adapter only, untested."""
    app_id = "kitty"

    def setup(self, runtime, mode, child):
        if mode == "default":
            return {}, [str(self.binary), *child]
        text = (f"font_family IBM Plex Mono\nfont_size {self.args.font_size:g}\nscrollback_lines 0\ncursor_blink_interval 0\n"
                "background_opacity 1.0\nupdate_check_interval 0\n")
        (runtime / "kitty.conf").write_text(text)
        return {"kitty.conf": text}, [str(self.binary), "--config", str(runtime / "kitty.conf"), *child]


# `ghostty-main` is the same adapter for a second Ghostty build (its main
# branch), measured beside the release.
ADAPTERS = {"hotaru": Hotaru, "ghostty": Ghostty, "ghostty-main": Ghostty, "gnome-terminal": GnomeTerminal, "konsole": Konsole,
            "kgx": Kgx, "foot": Foot, "alacritty": Alacritty, "kitty": Kitty}


def gpu_evidence(pids):
    """The GL, EGL and Vulkan libraries each process maps, and the DRM devices it holds."""
    evidence = {}
    keys = ("libEGL", "libGLES", "libGLX", "libGL.", "libOpenGL", "libvulkan", "libgallium", "radeonsi", "_dri.so", "libnvidia", "libgbm")
    for pid in pids:
        with contextlib.suppress(OSError):
            libraries = sorted({Path(line.split()[-1]).name for line in Path(f"/proc/{pid}/maps").read_text().splitlines()
                                if line.split()[-1].startswith("/") and any(k in Path(line.split()[-1]).name for k in keys)})
            devices = set()
            for fd in Path(f"/proc/{pid}/fd").iterdir():
                with contextlib.suppress(OSError):
                    target = os.readlink(fd)
                    if target.startswith(("/dev/dri/", "/dev/nvidia")):
                        devices.add(target)
            evidence[str(pid)] = {"comm": Path(f"/proc/{pid}/comm").read_text().strip(), "libraries": libraries,
                                  "devices": sorted(devices)}
    return evidence


def renderer_lines(log):
    """What GTK (GSK_DEBUG=renderer) and Ghostty say about how they draw."""
    text = read(log) or ""
    return [line for line in text.splitlines() if "Using renderer" in line or "Not using" in line or "EGL vendor" in line
            or "loaded OpenGL" in line or "CPU presentation" in line or "renderer=" in line]


def run(args):
    evidence = args.output.resolve()
    evidence.mkdir(parents=True, exist_ok=False)
    if args.observer:
        observer_binary, sampler_binary = args.observer / "frame-observer", args.observer / "ppt-sampler"
        shutil.copy(args.observer / "build.json", evidence / "observer-build.json")
    else:
        observer_binary, sampler_binary = build_observer(evidence / "observer-build", {
            "screencopy": Path(os.environ["SCREENCOPY_XML"]), "virtual-keyboard": Path(os.environ["VIRTUAL_KEYBOARD_XML"])})
    adapter = ADAPTERS[args.kind](args.binary, args)
    phase, mode = args.phase, ("default" if args.phase == "idle-default" else "matched")
    if args.runtime_dir:
        # The same path every trial, emptied first: caches keyed by paths
        # (KDE's service cache, for one) stay valid from the warm template.
        runtime = args.runtime_dir
        shutil.rmtree(runtime, ignore_errors=True)
        runtime.mkdir(mode=0o700)
        temporary = None
    else:
        temporary = tempfile.TemporaryDirectory(prefix="tb.", dir=args.runtime_parent)
        runtime = Path(temporary.name)
    os.chmod(runtime, 0o700)
    for name in ("home", "config", "cache", "data", "state", "config-dirs", "services", "dconf-keyfile"):
        (runtime / name).mkdir()
    if args.cache_template:
        shutil.copytree(args.cache_template, runtime / "cache", dirs_exist_ok=True)
    fonts = runtime / "fonts.conf"
    fonts.write_text('<?xml version="1.0"?><!DOCTYPE fontconfig SYSTEM "fonts.dtd"><fontconfig>' +
                     ''.join('<dir>' + escape(str(p)) + '</dir>' for p in (args.font_dir, args.fallback_dir)) +
                     '<cachedir>' + escape(str(args.fontconfig_cache or runtime / "cache/fontconfig")) + '</cachedir></fontconfig>')
    # An allowlist, not the host's environment: no host theme, input method,
    # GSettings overrides, Qt platform theme or system XDG configuration.
    env = {"PATH": os.environ["PATH"], "LANG": "C.UTF-8", "HOME": str(runtime / "home"), "XDG_RUNTIME_DIR": str(runtime),
           "XDG_CONFIG_HOME": str(runtime / "config"), "XDG_CACHE_HOME": str(runtime / "cache"),
           "XDG_DATA_HOME": str(runtime / "data"), "XDG_STATE_HOME": str(runtime / "state"),
           "XDG_CONFIG_DIRS": str(runtime / "config-dirs"), "XDG_CURRENT_DESKTOP": "sway", "XDG_SESSION_TYPE": "wayland",
           "FONTCONFIG_FILE": str(fonts), "__EGL_VENDOR_LIBRARY_FILENAMES": MESA_EGL,
           "VK_DRIVER_FILES": RADEON_VK, "VK_ICD_FILENAMES": RADEON_VK, "HOTARU_BENCH_FIXTURE_CPUS": args.fixture_cpus}
    if os.environ.get("LOCALE_ARCHIVE"):
        env["LOCALE_ARCHIVE"] = os.environ["LOCALE_ARCHIVE"]
    app_env = dict(adapter.env(runtime))
    if adapter.gtk4:
        app_env["GSK_DEBUG"] = "renderer"
    processes, logs, readers = [], [], []
    result = {"schema": 2, "status": "running", "kind": args.kind, "phase": phase, "mode": mode, "purpose": args.purpose,
              "scope": "private headless sway (GLES2 on the AMD iGPU) screencopy completion; no physical scanout or parser timing",
              "marker_contract": "matching complete top/bottom 76-cell markers, sequence32 + CRC32(sequence,nonce); sampled opaque interiors; not a full glyph-pixel CRC",
              "recorded_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(), "nonce": args.nonce,
              "capture_interval_us": args.capture_us, "commands": {}, "cpu_sets": {"app": args.app_cpus, "compositor": args.compositor_cpus,
                  "observer": args.observer_cpus, "fixture": args.fixture_cpus, "controller": args.controller_cpus},
              "host": os.uname().nodename, "kernel": os.uname().release, "clock_ticks_per_second": os.sysconf("SC_CLK_TCK"),
              "load_average_start": os.getloadavg(), "tool_sha256": source_hash(__file__), "binary": str(args.binary),
              "application_environment": app_env, "cache_template": str(args.cache_template) if args.cache_template else None,
              "environment": {k: v for k, v in env.items() if k not in ("PATH",)}, "gpu": {}, "windows": {}, "power": {},
              "fonts": {str(p): source_hash(p) for d in (args.font_dir, args.fallback_dir) for p in sorted(d.rglob('*'))
                        if p.is_file() and p.suffix in {'.ttf', '.otf', '.ttc'}}}
    ticks = result["clock_ticks_per_second"]
    power = None
    app = compositor = bus = None
    fixture_pid = None

    def launch(name, command, cpus=None, pipes=False, extra=None):
        log = (evidence / f"{name}.log").open("wb"); logs.append(log)
        if cpus:
            command = ["taskset", "-c", cpus, *command]
        result["commands"][name] = command
        process = subprocess.Popen(command, env=env | (extra or {}), stdin=subprocess.PIPE if pipes else subprocess.DEVNULL,
                                   stdout=subprocess.PIPE if pipes else log, stderr=log, start_new_session=True, bufsize=0)
        processes.append(process)
        return process

    def ours(table):
        """Processes started inside this trial's private session (their
        environment names its runtime directory), wherever they were
        reparented: a bus-activated server's parent isn't the bus."""
        marker_env = f"XDG_RUNTIME_DIR={runtime}".encode()
        found = []
        for pid in table:
            with contextlib.suppress(OSError):
                if marker_env in Path(f"/proc/{pid}/environ").read_bytes().split(b"\0"):
                    found.append(pid)
        return found

    def terminal_pids():
        """The terminal's processes: what was launched, and what it had the
        bus activate (gnome-terminal-server), without the PTY fixture."""
        table = parents()
        roots = [app.pid] if app else []
        if adapter.services:
            launched = {process.pid for process in processes}
            launched.update(descendants([pid for pid in (compositor.pid if compositor else 0, observer.pid if observer else 0) if pid], table))
            for pid in ours(table):
                command = read(f"/proc/{pid}/cmdline") or ""
                if pid not in launched and not any(name in command for name in
                                                   ("dbus-daemon", "dconf-service", "terminal-bench.py", "grim", "swaymsg")):
                    roots.append(pid)
        pids = descendants(roots, table)
        if fixture_pid:
            excluded = set(descendants([fixture_pid], table))
            pids = [p for p in pids if p not in excluded]
        return pids

    def roles():
        """Process groups whose CPU each window reports."""
        table = parents()
        return {"terminal": terminal_pids(), "compositor": descendants([compositor.pid], table) if compositor else [],
                "fixture": descendants([fixture_pid], table) if fixture_pid else [],
                "observer": [observer.pid] if observer else []}

    def sample_roles():
        return {name: group_sample(pids) for name, pids in roles().items()}

    def window(name, before, after, **extra):
        deltas = {role: group_delta(before[role], after[role], ticks) for role in before}
        entry = {"start_ns": before["terminal"]["t"], "end_ns": after["terminal"]["t"], "roles": deltas, **extra}
        result["windows"][name] = entry
        return entry

    def gpu_time():
        table = parents()
        return {"terminal": drm_ns(drm_clients(terminal_pids())),
                "compositor": drm_ns(drm_clients(descendants([compositor.pid], table)))}

    def timed_window(name, seconds, series_role="terminal"):
        """Idle for `seconds`, sampling every role at both ends and one role
        each second. The process list is fixed at the start, so the
        controller's own cost is the same in baseline and terminal windows."""
        gpu0 = gpu_time()
        before = sample_roles()
        pids = [int(pid) for pid in before[series_role]["procs"]]
        series = []
        end = time.monotonic() + seconds
        while time.monotonic() < end:
            time.sleep(min(1, max(0, end - time.monotonic())))
            now = group_sample(pids)
            delta = group_delta(before[series_role], now, ticks)
            series.append({"t": now["t"], "cpu_ms_sched": round(delta["cpu_ms_sched"], 3),
                           "ctxt": delta["ctxt_voluntary"] + delta["ctxt_involuntary"]})
        after = sample_roles()
        gpu1 = gpu_time()
        return window(name, before, after, gpu_ns={k: gpu1[k] - gpu0[k] for k in gpu0}, series_role=series_role, series=series)

    observer = None
    try:
        power = Power(sampler_binary, evidence / "power.tsv", args.controller_cpus)
        sway_config = runtime / "sway.conf"
        sway_config.write_text(
            "output HEADLESS-1 mode 1920x1440@60Hz\nseat seat0 fallback true\nfocus_follows_mouse no\ndefault_border none\n"
            "swaybg_command -\nswaynag_command -\nxwayland disable\n"
            f'for_window [app_id="{adapter.app_id}"] floating enable, border none, resize set {args.window_width} px {args.window_height} px, move position 20 px 20 px\n')
        compositor = launch("sway", ["sway", "--unsupported-gpu", "--verbose", "-c", str(sway_config)], args.compositor_cpus,
                            extra={"WLR_BACKENDS": "headless", "WLR_RENDERER": "gles2", "WLR_RENDER_DRM_DEVICE": args.render_node,
                                   "WLR_LIBINPUT_NO_DEVICES": "1",
                                   # The trial's bus (started next): Nixpkgs' sway wrapper would
                                   # otherwise start a session bus of its own with dbus-run-session.
                                   "DBUS_SESSION_BUS_ADDRESS": "unix:path=" + str(runtime / "bus")})
        display = wait("private Wayland", lambda: next((p for p in runtime.glob("wayland-*") if p.is_socket()), None))
        sway_socket = wait("private Sway", lambda: next(runtime.glob("sway-ipc.*.sock"), None))
        env.update(WAYLAND_DISPLAY=display.name, SWAYSOCK=str(sway_socket))
        logs[-1].flush()
        result["gpu"]["compositor_log"] = [line.split("] ", 2)[-1] for line in (evidence / "sway.log").read_text(errors="replace").splitlines()
                                           if any(key in line for key in ("EGL vendor", "GL vendor", "GL renderer", "Using OpenGL", "Creating GLES2"))]
        if not any("GL renderer" in line for line in result["gpu"]["compositor_log"]):
            raise RuntimeError("the private compositor did not report a GPU renderer")
        # The session bus, after the compositor so activated services see it;
        # it runs on the application's CPUs, which an activated server inherits.
        if adapter.dconf:
            keyfile = adapter.dconf_keyfile(mode)
            result["dconf_keyfile"] = keyfile
            if keyfile:
                (runtime / "dconf-keyfile/settings").write_text(keyfile)
                (runtime / "config/dconf").mkdir()
                subprocess.run(["dconf", "compile", str(runtime / "config/dconf/user"), str(runtime / "dconf-keyfile")], check=True)
            (runtime / "services/ca.desrt.dconf.service").write_text(
                f"[D-BUS Service]\nName=ca.desrt.dconf\nExec={args.dconf_service}\n")
        for service in adapter.services:
            shutil.copy(args.binary / f"share/dbus-1/services/{service}.service", runtime / "services")
        address = "unix:path=" + str(runtime / "bus")
        (runtime / "dbus.conf").write_text(
            '<busconfig><type>session</type><listen>' + escape(address) + '</listen><auth>EXTERNAL</auth>'
            '<servicedir>' + escape(str(runtime / "services")) + '</servicedir>'
            '<policy context="default"><allow send_destination="*"/><allow receive_sender="*"/><allow own="*"/></policy></busconfig>')
        bus = launch("dbus", ["dbus-daemon", "--nofork", "--nopidfile", "--config-file=" + str(runtime / "dbus.conf")], args.app_cpus,
                     extra=app_env)
        wait("private D-Bus", lambda: (runtime / "bus").is_socket())
        env["DBUS_SESSION_BUS_ADDRESS"] = address
        result["dbus_services"] = sorted(p.name for p in (runtime / "services").iterdir())

        def sway(command):
            return json.loads(subprocess.check_output(["swaymsg", "-s", str(sway_socket), *command], env=env, timeout=3))

        observer = launch("observer", [str(observer_binary)], args.observer_cpus, pipes=True)
        observations = JsonReader(observer.stdout, evidence / "observer.jsonl"); readers.append(observations)
        observations.until(lambda e: e["event"] == "ready")

        def observe(command):
            observer.stdin.write((command + "\n").encode()); observer.stdin.flush()

        time.sleep(.5)
        if phase in ("energy", "idle-default", "baseline"):
            timed_window("baseline_pre", args.baseline_seconds, "compositor")
        if phase == "baseline":
            # No terminal: the same compositor and observer, idle, for a
            # second stretch standing in for the trials' idle windows.
            timed_window("baseline_idle", args.idle_seconds, "compositor")
            result["status"] = "passed"
            return
        observe(f"start {args.nonce} {args.capture_us}")
        control = str(runtime / "fixture.sock")
        with socket.socket(socket.AF_UNIX) as listener:
            listener.bind(control); listener.listen(1); listener.settimeout(45)
            child = [sys.executable, str(Path(__file__).resolve()), "--fixture", control, str(args.nonce)]
            files, command = adapter.setup(runtime, mode, child)
            result["configuration"] = files
            launch_ns = time.monotonic_ns()
            app = launch("application", command, args.app_cpus, extra=app_env)
            connection, _ = listener.accept()
            with connection:
                stream = connection.makefile("rwb", buffering=0)
                fixtures = JsonReader(stream, evidence / "fixture.jsonl"); readers.append(fixtures)
                ready = fixtures.until(lambda e: e["event"] == "ready")
                fixture_pid = ready["pid"]
                fixtures.until(lambda e: e["event"] == "write" and e["sequence"] == 0)
                startup_frame = observations.until(lambda e: e["event"] == "capture" and e["valid"] and e["sequence"] == 0,
                                                   timeout=args.startup_timeout)
                startup_write = [e for e in fixtures.events if e["event"] == "write" and e["sequence"] == 0
                                 and e["write_start_ns"] <= startup_frame["ready_ns"]][-1]
                startup_geometry = [e for e in observations.events if e["event"] == "geometry"][-1]
                result["startup"] = {"launch_ns": launch_ns, "pty_ready": ready, "fixture_write": startup_write,
                                     "matching_frame": startup_frame, "geometry": startup_geometry,
                                     "launch_to_pty_ready_ns": ready["ready_ns"] - launch_ns,
                                     "launch_to_matching_capture_ns": startup_frame["ready_ns"] - launch_ns,
                                     "initial_grid": [startup_write["cols"], startup_write["rows"]],
                                     "initial_grid_matches_requested": (startup_write["cols"], startup_write["rows"], startup_geometry["rows"])
                                                                       == (args.cols, args.rows, args.rows)}
                with contextlib.suppress(ProcessLookupError, FileNotFoundError):
                    result["running_executable"] = str(Path(f"/proc/{app.pid}/exe").resolve())
                    result["running_executable_sha256"] = source_hash(f"/proc/{app.pid}/exe")
                result["startup"]["power"] = None  # filled in after the sampler stops
                window_node = wait("mapped app", lambda: next((n for n in nodes(sway(["-t", "get_tree"])) if n.get("app_id") == adapter.app_id), None))
                criteria = f"[con_id={window_node['id']}]"
                sway([f"{criteria} focus"])
                width, height = args.window_width, args.window_height
                if mode == "matched":
                    for _ in range(100):
                        send(stream, {"operation": "size"}); size = fixtures.until(lambda e: e["event"] == "size")
                        if (size["cols"], size["rows"]) == (args.cols, args.rows):
                            break
                        width += max(-60, min(60, (args.cols - size["cols"]) * 3))
                        height += max(-60, min(60, (args.rows - size["rows"]) * 3))
                        if not 100 <= width <= 1870 or not 100 <= height <= 1390:
                            raise RuntimeError("target grid exceeds private output")
                        sway([f"{criteria} resize set {width} px {height} px"]); time.sleep(.03)
                    else:
                        raise RuntimeError("grid calibration failed")
                    observe("reset")
                    send(stream, {"operation": "paint", "sequence": 1, "kind": "initial"})
                    fixtures.until(lambda e: e["event"] == "write" and e["sequence"] == 1)
                    observations.until(lambda e: e["event"] == "capture" and e["valid"] and e["sequence"] == 1)
                    geometry = [e for e in observations.events if e["event"] == "geometry"][-1]
                else:
                    send(stream, {"operation": "size"}); size = fixtures.until(lambda e: e["event"] == "size")
                    geometry = startup_geometry
                tree = sway(["-t", "get_tree"])
                node = next(n for n in nodes(tree) if n.get("app_id") == adapter.app_id)
                result.update(grid=size, window_pixels=[width, height], geometry=geometry,
                              window_rect=node.get("rect"), window_focused=node.get("focused"),
                              compositor_outputs=sway(["-t", "get_outputs"]))
                cell = (geometry["cell_width"], geometry["cell_height"])
                result["cell_geometry_matches_required"] = (not args.cell_width) or cell == (args.cell_width, args.cell_height)
                if mode == "matched" and args.cell_width and not result["cell_geometry_matches_required"]:
                    raise RuntimeError(f"cell geometry {cell} differs from the calibration's {(args.cell_width, args.cell_height)}")
                result["gpu"]["application"] = gpu_evidence(terminal_pids())
                table = parents()
                result["session_processes"] = {str(pid): {"ppid": table.get(pid), "comm": (read(f"/proc/{pid}/comm") or "").strip()}
                                               for pid in ours(table)}
                result["terminal_processes"] = {str(pid): {"comm": (read(f"/proc/{pid}/comm") or "").strip(),
                                                           "exe": os.path.realpath(f"/proc/{pid}/exe")} for pid in terminal_pids()}
                result["terminal_executables_sha256"] = sorted({source_hash(p["exe"]) for p in result["terminal_processes"].values()
                                                                if os.path.exists(p["exe"])})
                logs[-1].flush()
                result["gpu"]["renderer_log"] = renderer_lines(evidence / "application.log")

                if phase in ("startup", "calibrate"):
                    observe("stop"); time.sleep(.15)
                    time.sleep(args.settle_seconds)
                    result["memory_after_startup"] = memory_sample(terminal_pids())
                    result["memory_compositor"] = memory_sample(descendants([compositor.pid]))

                if phase == "steady":
                    send(stream, {"operation": "arm", "start": 100, "count": args.keys})
                    fixtures.until(lambda e: e["event"] == "armed")
                    rng = random.Random(0x29312982)
                    keyboard = []
                    for sequence in range(100, 100 + args.keys):
                        time.sleep(rng.uniform(.04, .08))
                        observe(f"key {sequence} 30")  # US evdev KEY_A; actual Wayland keyboard events.
                        injected = observations.until(lambda e: e["event"] == "key" and e["sequence"] == sequence)
                        written = fixtures.until(lambda e: e["event"] == "write" and e["sequence"] == sequence)
                        seen = observations.until(lambda e: e["event"] == "capture" and e["valid"] and e["sequence"] == sequence)
                        if not injected["flush_complete"]:
                            raise RuntimeError("virtual input was transport-backpressured; timestamp is not a complete flush")
                        keyboard.append({"sequence": sequence, "inject_ns": injected["inject_ns"], "read_ns": written["read_ns"],
                                         "write_end_ns": written["write_end_ns"], "capture_ns": seen["ready_ns"],
                                         "inject_to_fixture_read_ns": written["read_ns"] - injected["inject_ns"],
                                         "fixture_write_to_capture_ns": seen["ready_ns"] - written["write_end_ns"],
                                         "inject_to_capture_ns": seen["ready_ns"] - injected["inject_ns"]})
                    result["keyboard"] = keyboard
                    latencies = [e["inject_to_capture_ns"] for e in keyboard]
                    result["keyboard_summary_ns"] = {name: percentile(latencies, q) for name, q in [("p50", .5), ("p95", .95), ("p99", .99), ("max", 1)]}
                    result["streams"] = []
                    for index, (kind, hz) in enumerate([(kind, hz) for kind in ("marker", "sparse", "scroll") for hz in (30, 60, 0)]):
                        first = 10000 + index * 1000; last = first + args.frames - 1
                        before = sample_roles()
                        send(stream, {"operation": "stream", "start": first, "count": args.frames, "kind": kind, "hz": hz})
                        fixtures.until(lambda e: e["event"] == "stream_done")
                        observations.until(lambda e: e["event"] == "capture" and e["valid"] and e["sequence"] == last)
                        after = sample_roles()
                        writes = [e for e in fixtures.events if e["event"] == "write" and first <= e["sequence"] <= last]
                        seen = [e for e in observations.events if e["event"] == "capture" and e["valid"] and first <= e["sequence"] <= last]
                        if any(a["sequence"] > b["sequence"] for a, b in zip(seen, seen[1:])):
                            raise RuntimeError("observed complete frame sequence went backwards")
                        unique = {}
                        for event in seen:
                            unique.setdefault(event["sequence"], event)
                        result["streams"].append({"kind": kind, "producer_hz": hz, "produced": len(writes), "observed_unique": len(unique),
                            "unobserved_or_coalesced": args.frames - len(unique), "first_write_ns": writes[0]["write_start_ns"],
                            "last_write_ns": writes[-1]["write_end_ns"], "last_capture_ns": unique[last]["ready_ns"],
                            "final_write_to_capture_ns": unique[last]["ready_ns"] - writes[-1]["write_end_ns"],
                            "bytes": sum(e["bytes"] for e in writes),
                            "roles": {role: group_delta(before[role], after[role], ticks) for role in before}})
                    observe("stop")

                if phase == "throughput":
                    result["throughput"] = []
                    for index, corpus in enumerate(args.throughput):
                        sequence = 50000 + index
                        # The observer stops during the flood, so its captures
                        # add no compositor or GPU work to the energy window;
                        # it restarts when the writes end, to time the frame
                        # that shows the closing marker.
                        observe("stop"); time.sleep(.15)
                        pre = timed_window(f"tp_pre:{corpus.stem}", args.throughput_pre_seconds)
                        gpu0 = gpu_time()
                        before = sample_roles()
                        send(stream, {"operation": "throughput", "path": str(corpus.resolve()), "repeat": args.throughput_repeat,
                                      "sequence": sequence})
                        done = fixtures.until(lambda e: e["event"] == "throughput_done", timeout=900)
                        observe(f"start {args.nonce} {args.capture_us}")
                        seen = observations.until(lambda e: e["event"] == "capture" and e["valid"] and e["sequence"] == sequence,
                                                  timeout=300)
                        after = sample_roles()
                        gpu1 = gpu_time()
                        consumed = done["write_end_ns"] - done["write_start_ns"]
                        result["throughput"].append({
                            "corpus": str(corpus), "corpus_sha256": source_hash(corpus), "repeat": args.throughput_repeat,
                            "bytes": done["bytes"], "write_start_ns": done["write_start_ns"], "write_end_ns": done["write_end_ns"],
                            "final_capture_ns": seen["ready_ns"], "consume_ns": consumed,
                            "mib_per_second": done["bytes"] / 2**20 / (consumed / 1e9),
                            "to_final_capture_ns": seen["ready_ns"] - done["write_start_ns"],
                            "roles": {role: group_delta(before[role], after[role], ticks) for role in before},
                            "gpu_ns": {k: gpu1[k] - gpu0[k] for k in gpu0}, "pre_window": f"tp_pre:{corpus.stem}"})
                    observe("stop"); time.sleep(.15)
                    time.sleep(args.settle_seconds)
                    result["memory_after_throughput"] = memory_sample(terminal_pids())
                    result["memory_compositor"] = memory_sample(descendants([compositor.pid]))

                if phase in ("energy", "idle-default"):
                    send(stream, {"operation": "prompt"})
                    fixtures.until(lambda e: e["event"] == "prompt")
                    observe("stop"); time.sleep(.15)
                    time.sleep(args.settle_seconds)
                    result["memory_before_idle"] = memory_sample(terminal_pids())
                    timed_window("idle", args.idle_seconds)
                    if phase == "energy":
                        send(stream, {"operation": "prompt", "echo": True})
                        fixtures.until(lambda e: e["event"] == "prompt")
                        time.sleep(1)
                        gpu0 = gpu_time()
                        before = sample_roles()
                        start = time.monotonic_ns()
                        period = 1e9 / args.typing_hz
                        presses = int(args.typing_seconds * args.typing_hz)
                        for index in range(presses):
                            deadline = start + int(index * period)
                            delay = (deadline - time.monotonic_ns()) / 1e9
                            if delay > 0:
                                time.sleep(delay)
                            observe(f"key {200000 + index} 30")
                        delay = (start + int(presses * period) - time.monotonic_ns()) / 1e9
                        if delay > 0:
                            time.sleep(delay)
                        after = sample_roles()
                        gpu1 = gpu_time()
                        keys = [e for e in observations.events if e["event"] == "key" and e["sequence"] >= 200000]
                        window("typing", before, after, presses=presses, hz=args.typing_hz,
                               flushed=sum(1 for e in keys if e["flush_complete"]), gpu_ns={k: gpu1[k] - gpu0[k] for k in gpu0})
                        time.sleep(2)
                    result["memory_after_idle"] = memory_sample(terminal_pids())
                    result["memory_compositor"] = memory_sample(descendants([compositor.pid]))

                subprocess.run(["grim", str(evidence / "final.png")], env=env, check=True, timeout=5)
                logs[-1].flush()
                result["gpu"]["renderer_log"] = renderer_lines(evidence / "application.log")
                if phase in ("energy", "idle-default"):
                    # The terminal goes; the compositor and observer stay idle.
                    for pid in terminal_pids():
                        with contextlib.suppress(ProcessLookupError):
                            os.kill(pid, signal.SIGTERM)
                    wait("terminal exit", lambda: (app.poll(), not terminal_pids())[1], timeout=10)
                    app = None
                    time.sleep(1)
                    timed_window("baseline_post", args.baseline_post_seconds, "compositor")
                result["status"] = "passed"
    except Exception as error:
        result.update(status="failed", error=repr(error))
        with contextlib.suppress(Exception):
            subprocess.run(["grim", str(evidence / "failure.png")], env=env, timeout=5)
        raise
    finally:
        result["load_average_end"] = os.getloadavg()
        with contextlib.suppress(Exception):
            if bus:
                for pid in descendants([bus.pid])[1:]:
                    with contextlib.suppress(ProcessLookupError):
                        os.kill(pid, signal.SIGTERM)
        for process in reversed(processes):
            if process.poll() is None:
                with contextlib.suppress(ProcessLookupError): os.killpg(process.pid, signal.SIGTERM)
                try: process.wait(timeout=3)
                except subprocess.TimeoutExpired:
                    with contextlib.suppress(ProcessLookupError): os.killpg(process.pid, signal.SIGKILL)
                    process.wait(timeout=3)
        with contextlib.suppress(Exception):
            if bus:
                for pid in descendants([bus.pid]):
                    with contextlib.suppress(ProcessLookupError):
                        os.kill(pid, signal.SIGKILL)
        for reader in readers:
            reader.thread.join(timeout=2); reader.file.close()
        for log in logs: log.close()
        if power:
            with contextlib.suppress(Exception):
                power.stop()
                for name, entry in result["windows"].items():
                    result["power"][name] = power.window(name, entry["start_ns"], entry["end_ns"])
                for item in result.get("throughput", []):
                    result["power"]["tp:" + Path(item["corpus"]).stem] = power.window(
                        "tp:" + Path(item["corpus"]).stem, item["write_start_ns"], item["final_capture_ns"])
                if "startup" in result:
                    s = result["startup"]
                    s["power"] = power.window("startup", s["launch_ns"], s["matching_frame"]["ready_ns"])
        (evidence / "result.json").write_text(json.dumps(result, indent=1) + "\n")
        if args.save_cache:
            # Caches the run left (shader, service and font caches) become the
            # warm cache later trials start from.
            shutil.rmtree(args.save_cache, ignore_errors=True)
            shutil.copytree(runtime / "cache", args.save_cache, symlinks=True)
        if temporary:
            temporary.cleanup()
        else:
            shutil.rmtree(runtime, ignore_errors=True)
    print(json.dumps({"status": result["status"], "output": str(evidence)}))


def main():
    if len(sys.argv) == 4 and sys.argv[1] == "--fixture":
        fixture(sys.argv[2], int(sys.argv[3])); return
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("kind", choices=sorted(ADAPTERS)); parser.add_argument("binary", type=Path)
    parser.add_argument("--phase", choices=("calibrate", "startup", "steady", "throughput", "energy", "idle-default", "baseline"),
                        required=True)
    parser.add_argument("--purpose", choices=("qualification", "measurement"), default="qualification")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--observer", type=Path, help="directory with a prebuilt frame-observer and ppt-sampler (build.json)")
    parser.add_argument("--runtime-parent", default="/tmp", help="short parent for the private runtime directory")
    parser.add_argument("--runtime-dir", type=Path, help="a fixed private runtime directory, emptied and recreated (short path)")
    parser.add_argument("--font-dir", type=Path, required=True); parser.add_argument("--fallback-dir", type=Path, required=True)
    parser.add_argument("--fontconfig-cache", type=Path, help="a shared fontconfig cache directory (warm), instead of a fresh one")
    parser.add_argument("--cache-template", type=Path, help="copied into the fresh XDG_CACHE_HOME first (warm shader/service caches)")
    parser.add_argument("--save-cache", type=Path, help="copy the run's XDG_CACHE_HOME here afterwards (a cache template)")
    parser.add_argument("--render-node", default="/dev/dri/renderD128")
    parser.add_argument("--dconf-service", default=os.environ.get("TERMBENCH_DCONF_SERVICE", ""))
    parser.add_argument("--throughput", action="append", default=[], type=Path, metavar="CORPUS")
    parser.add_argument("--throughput-repeat", type=int, default=8)
    parser.add_argument("--throughput-pre-seconds", type=float, default=4)
    parser.add_argument("--scrollback", choices=("zero", "ghostty-default"), default="zero",
                        help="history in the matched configuration (see the adapters' comment)")
    parser.add_argument("--cols", type=int, default=120); parser.add_argument("--rows", type=int, default=40)
    parser.add_argument("--font-size", type=float, default=14.0)
    parser.add_argument("--window-width", type=int, default=1400); parser.add_argument("--window-height", type=int, default=1080)
    parser.add_argument("--cell-width", type=int); parser.add_argument("--cell-height", type=int)
    parser.add_argument("--keys", type=int, default=32); parser.add_argument("--frames", type=int, default=60)
    parser.add_argument("--capture-us", type=int, default=1000)
    parser.add_argument("--settle-seconds", type=float, default=2)
    parser.add_argument("--baseline-seconds", type=float, default=20); parser.add_argument("--baseline-post-seconds", type=float, default=10)
    parser.add_argument("--idle-seconds", type=float, default=60)
    parser.add_argument("--typing-seconds", type=float, default=60); parser.add_argument("--typing-hz", type=float, default=10)
    parser.add_argument("--startup-timeout", type=float, default=30)
    parser.add_argument("--nonce", type=lambda value: int(value, 0), default=0x29312982)
    parser.add_argument("--app-cpus", default="2-5"); parser.add_argument("--compositor-cpus", default="6,14"); parser.add_argument("--observer-cpus", default="7")
    parser.add_argument("--controller-cpus", default="15"); parser.add_argument("--fixture-cpus", default="1")
    parser.add_argument("--lock", type=Path, help="private lock file held for the run (the matrix gives each trial its own)")
    args = parser.parse_args()
    if not (76 <= args.cols <= 150 and 4 <= args.rows <= 50 and 1 <= args.keys <= 2000 and 1 <= args.frames <= 500 and
            1000 <= args.capture_us <= 1000000 and 0 <= args.nonce <= 0xffffffff):
        parser.error("argument outside bounded benchmark limits")
    for name in ("binary", "font_dir", "fallback_dir"):
        setattr(args, name, getattr(args, name).resolve(strict=True))
    if args.kind == "gnome-terminal" and args.binary.name == "gnome-terminal":
        args.binary = args.binary.parent.parent  # the package: its client, server and D-Bus service file
    cpu_masks = [cpu_set(getattr(args, name + "_cpus")) for name in ("app", "compositor", "observer", "controller", "fixture")]
    available = os.sched_getaffinity(0)
    if any(not mask <= available for mask in cpu_masks) or sum(map(len, cpu_masks)) != len(set.union(*cpu_masks)):
        parser.error("benchmark CPU sets must be available and disjoint")
    os.sched_setaffinity(0, cpu_set(args.controller_cpus))
    if args.lock:
        with args.lock.open("a") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX)
            run(args)
    else:
        run(args)


if __name__ == "__main__":
    main()
