#!/usr/bin/env python3
"""Observe complete PTY sequence/CRC markers through a private Wayland compositor.

Software observation only. Capture completion is not physical scanout or a
terminal parser acknowledgement. Each invocation measures one isolated app;
alternate applications externally and qualify identical physical cell geometry.
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
    # Markers bracket the byte stream. Their CRC covers sequence+run nonce,
    # not arbitrary glyph pixels: this is explicitly recorded in each report.
    return bytes(data)


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
                      "cpu_affinity": sorted(os.sched_getaffinity(0))})
        armed = []

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
                elif operation == "quit":
                    return
                else:
                    raise RuntimeError(f"unknown fixture operation {operation}")
            if 0 in readable:
                data = os.read(0, 4096)
                read_ns = time.monotonic_ns()
                if not data:
                    return
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


def source_hash(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def build_observer(directory):
    directory.mkdir(parents=True, exist_ok=True)
    registry = Path.home() / ".cargo/registry/src"
    patterns = {"screencopy": "*/wayland-protocols-wlr-*/wlr-protocols/unstable/wlr-screencopy-unstable-v1.xml",
                "virtual-keyboard": "*/wayland-protocols-misc-*/protocols/virtual-keyboard-unstable-v1.xml"}
    protocols = {}
    sources = []
    for name, pattern in patterns.items():
        candidates = sorted(registry.glob(pattern))
        if not candidates:
            raise RuntimeError(f"missing protocol XML: {pattern}")
        xml = candidates[-1]
        protocols[name] = {"path": str(xml), "sha256": source_hash(xml)}
        header, code = directory / f"{name}-client.h", directory / f"{name}-protocol.c"
        subprocess.run(["wayland-scanner", "client-header", str(xml), str(header)], check=True)
        subprocess.run(["wayland-scanner", "private-code", str(xml), str(code)], check=True)
        sources.append(str(code))
    flags = subprocess.check_output(["pkg-config", "--cflags", "--libs", "wayland-client", "xkbcommon"], text=True).split()
    source = Path(__file__).with_name("frame-observer.c")
    binary = directory / "frame-observer"
    command = ["cc", "-O2", "-std=c11", "-Wall", "-Wextra", "-Werror", "-I" + str(directory), str(source), *sources, *flags, "-o", str(binary)]
    subprocess.run(command, check=True)
    metadata = {"command": command, "source_sha256": source_hash(source), "binary_sha256": source_hash(binary),
                "protocols": protocols, "compiler": subprocess.check_output(["cc", "--version"], text=True).splitlines()[0],
                "pkg_config_versions": subprocess.check_output(["pkg-config", "--modversion", "wayland-client", "xkbcommon"], text=True).splitlines()}
    (directory / "build.json").write_text(json.dumps(metadata, indent=2) + "\n")
    return binary


def nodes(node):
    yield node
    for child in node.get("nodes", []) + node.get("floating_nodes", []):
        yield from nodes(child)


def process_sample(pid):
    stat = Path(f"/proc/{pid}/stat").read_text().rsplit(")", 1)[1].split()
    status = Path(f"/proc/{pid}/status").read_text()
    memory = {}
    for line in status.splitlines():
        if line.split(":", 1)[0] in ("VmRSS", "VmHWM", "RssAnon", "RssFile", "Threads"):
            memory[line.split(":", 1)[0]] = int(line.split()[1])
    return {"sample_ns": time.monotonic_ns(), "cpu_ticks": int(stat[11]) + int(stat[12]),
            "faults_minor": int(stat[7]), "faults_major": int(stat[9]), "memory_kib_except_threads": memory}


def percentile(values, fraction):
    if not values:
        return None
    return sorted(values)[min(len(values) - 1, int((len(values) - 1) * fraction))]


def startup_geometry_matches(written, observed, cols, rows):
    return (written["cols"], written["rows"], observed["rows"]) == (cols, rows, rows)


def unrelated_uta_builds():
    found = []
    for proc in Path("/proc").iterdir():
        if not proc.name.isdecimal():
            continue
        try:
            args = (proc / "cmdline").read_bytes().split(b"\0")
            if args and Path(os.fsdecode(args[0])).name == "nix" and b"build" in args and any(
                    b"#uta" in arg or b"#checks.x86_64-linux.uta" in arg for arg in args):
                found.append({"pid": int(proc.name), "command": "nix build (Uta targets active)"})
        except (OSError, ValueError):
            pass
    return found


def run(args):
    evidence = args.output.resolve()
    evidence.mkdir(parents=True, exist_ok=False)
    observer_binary = build_observer(evidence / "observer-build")
    temporary = tempfile.TemporaryDirectory(prefix="hotaru-frames-")
    runtime = Path(temporary.name)
    os.chmod(runtime, 0o700)
    env = dict(os.environ)
    for name in list(env):
        if name.startswith(("SIDESTEP_", "GDK_", "GTK_", "AIZOME_")):
            env.pop(name, None)
    for name in ("DISPLAY", "WAYLAND_DISPLAY", "SWAYSOCK", "DBUS_SESSION_BUS_ADDRESS", "AIZOME_SHELL_SOCKET",
                 "AIZOME_SOCKET", "AIZOME_KIT_INSPECT", "SIDESTEP_BACKEND", "FONTCONFIG_FILE", "FONTCONFIG_PATH", "LD_LIBRARY_PATH"):
        env.pop(name, None)
    for name in ("home", "config", "cache", "data"):
        (runtime / name).mkdir()
    env.update(HOME=str(runtime / "home"), XDG_RUNTIME_DIR=str(runtime), XDG_CONFIG_HOME=str(runtime / "config"),
               XDG_CACHE_HOME=str(runtime / "cache"), XDG_DATA_HOME=str(runtime / "data"), XDG_CURRENT_DESKTOP="sway",
               XDG_SESSION_TYPE="wayland", WLR_BACKENDS="headless", WLR_RENDERER="pixman", WLR_LIBINPUT_NO_DEVICES="1",
               LIBGL_ALWAYS_SOFTWARE="1", GALLIUM_DRIVER="llvmpipe", AIZOME_FONT_DIR=str(args.font_dir), HOTARU_FONT_DIR=str(args.fallback_dir),
               HOTARU_BENCH_FIXTURE_CPUS=args.fixture_cpus, RUST_LOG="warn", LP_NUM_THREADS="4", SIDESTEP_RASTER_THREADS="4",
               AIZOME_NO_PIXEL_SCROLL=args.no_pixel_scroll)
    fonts = runtime / "fonts.conf"
    fonts.write_text('<?xml version="1.0"?><!DOCTYPE fontconfig SYSTEM "fonts.dtd"><fontconfig>' +
                     ''.join('<dir>' + escape(str(p)) + '</dir>' for p in (args.font_dir, args.fallback_dir)) +
                     '<cachedir>' + escape(str(runtime / "cache/fonts")) + '</cachedir></fontconfig>')
    env["FONTCONFIG_FILE"] = str(fonts)
    if args.mesa:
        env.update(__EGL_VENDOR_LIBRARY_FILENAMES=str(args.mesa / "share/glvnd/egl_vendor.d/50_mesa.json"),
                   LIBGL_DRIVERS_PATH=str(args.mesa / "lib/dri"))
    processes, logs, readers = [], [], []
    result = {"schema": 1, "status": "running", "kind": args.kind, "purpose": args.purpose,
              "scope": "private headless software compositor screencopy completion; no physical scanout or parser timing",
              "marker_contract": "matching complete top/bottom 76-cell markers, sequence32 + CRC32(sequence,nonce); sampled opaque interiors; not a full glyph-pixel CRC",
              "recorded_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(), "nonce": args.nonce,
              "capture_interval_us": args.capture_us, "commands": {}, "cpu_sets": {"app": args.app_cpus, "compositor": args.compositor_cpus,
                  "observer": args.observer_cpus, "fixture": args.fixture_cpus, "controller": args.controller_cpus},
              "host": os.uname().nodename, "kernel": os.uname().release, "clock_ticks_per_second": os.sysconf("SC_CLK_TCK"),
              "load_average": os.getloadavg(), "tool_sha256": source_hash(__file__), "binary_sha256": source_hash(args.binary),
              "environment": {name: env.get(name) for name in ["WLR_BACKENDS", "WLR_RENDERER", "LIBGL_ALWAYS_SOFTWARE", "GALLIUM_DRIVER",
                  "LP_NUM_THREADS", "SIDESTEP_RASTER_THREADS", "AIZOME_NO_PIXEL_SCROLL", "__EGL_VENDOR_LIBRARY_FILENAMES", "LIBGL_DRIVERS_PATH", "RUST_LOG"]},
              "fonts": {str(p): source_hash(p) for d in (args.font_dir, args.fallback_dir) for p in d.rglob('*') if p.is_file() and p.suffix in {'.ttf', '.otf', '.ttc'}}}
    result["unrelated_uta_builds"] = unrelated_uta_builds()

    def launch(name, command, cpus=None, pipes=False):
        log = (evidence / f"{name}.log").open("wb"); logs.append(log)
        if cpus:
            command = ["taskset", "-c", cpus, *command]
        result["commands"][name] = command
        process = subprocess.Popen(command, env=env, stdin=subprocess.PIPE if pipes else subprocess.DEVNULL,
                                   stdout=subprocess.PIPE if pipes else log, stderr=log, start_new_session=True, bufsize=0)
        processes.append(process)
        return process

    try:
        if args.purpose == "measurement" and result["unrelated_uta_builds"]:
            raise RuntimeError("unrelated Uta build is active; comparison timing must wait")
        bus_config = runtime / "dbus.conf"
        address = "unix:path=" + str(runtime / "dbus")
        bus_config.write_text('<busconfig><type>session</type><listen>' + escape(address) + '</listen><auth>EXTERNAL</auth>'
                             '<policy context="default"><allow send_destination="*"/><allow receive_sender="*"/><allow own="*"/></policy></busconfig>')
        launch("dbus", ["dbus-daemon", "--nofork", "--nopidfile", "--config-file=" + str(bus_config)])
        wait("private D-Bus", lambda: (runtime / "dbus").is_socket())
        env["DBUS_SESSION_BUS_ADDRESS"] = address
        sway_config = runtime / "sway.conf"
        app_id = "com.mitchellh.ghostty" if args.kind == "ghostty" else "org.aizome.Hotaru"
        sway_config.write_text("output HEADLESS-1 mode 1920x1440@60Hz\nseat seat0 fallback true\nfocus_follows_mouse no\ndefault_border none\n" +
            f'for_window [app_id="{app_id}"] floating enable, border none, resize set {args.window_width} px {args.window_height} px, move position 20 px 20 px\n')
        compositor = launch("sway", ["sway", "--unsupported-gpu", "-c", str(sway_config)], args.compositor_cpus)
        display = wait("private Wayland", lambda: next((p for p in runtime.glob("wayland-*") if p.is_socket()), None))
        sway_socket = wait("private Sway", lambda: next(runtime.glob("sway-ipc.*.sock"), None))
        env.update(WAYLAND_DISPLAY=display.name, SWAYSOCK=str(sway_socket))

        def sway(command):
            return json.loads(subprocess.check_output(["swaymsg", "-s", str(sway_socket), *command], env=env, timeout=3))

        observer = launch("observer", [str(observer_binary)], args.observer_cpus, pipes=True)
        observations = JsonReader(observer.stdout, evidence / "observer.jsonl"); readers.append(observations)
        observations.until(lambda e: e["event"] == "ready")

        def observe(command):
            observer.stdin.write((command + "\n").encode()); observer.stdin.flush()

        observe(f"start {args.nonce} {args.capture_us}")
        control = str(runtime / "fixture.sock")
        with socket.socket(socket.AF_UNIX) as listener:
            listener.bind(control); listener.listen(1); listener.settimeout(45)
            child = [sys.executable, str(Path(__file__).resolve()), "--fixture", control, str(args.nonce)]
            if args.kind == "ghostty":
                config_text = ("gtk-single-instance = false\nshell-integration = none\nscrollback-limit-bytes = 0\nscrollback-limit-lines = 0\n"
                               f"font-family = IBM Plex Mono\nfont-size = {args.font_size}\ncursor-style-blink = false\nbackground-opacity = 1\n")
                if args.ghostty_cell_height:
                    config_text += f"adjust-cell-height = {args.ghostty_cell_height}\n"
                config = runtime / "ghostty.conf"; config.write_text(config_text)
                command = [str(args.binary), "--config-default-files=false", "--config-file=" + str(config), "-e", *child]
                app_id = "com.mitchellh.ghostty"
            else:
                config_text = f'font_size = {args.font_size}\nscrollback = 0\ntheme = "night"\nshell_integration = false\n'
                config = runtime / "hotaru.toml"; config.write_text(config_text)
                command = [str(args.binary), "--config-file", str(config), "--", *child]
                app_id = "org.aizome.Hotaru"
            result["configuration"] = config_text
            launch_ns = time.monotonic_ns()
            app = launch("application", command, args.app_cpus)
            connection, _ = listener.accept()
            with connection:
                stream = connection.makefile("rwb", buffering=0)
                fixtures = JsonReader(stream, evidence / "fixture.jsonl"); readers.append(fixtures)
                ready = fixtures.until(lambda e: e["event"] == "ready")
                fixtures.until(lambda e: e["event"] == "write" and e["sequence"] == 0)
                startup_frame = observations.until(lambda e: e["event"] == "capture" and e["valid"] and e["sequence"] == 0)
                startup_write = [e for e in fixtures.events if e["event"] == "write" and e["sequence"] == 0
                                 and e["write_start_ns"] <= startup_frame["ready_ns"]][-1]
                startup_geometry = [e for e in observations.events if e["event"] == "geometry"][-1]
                result["startup"] = {"launch_ns": launch_ns, "pty_ready": ready, "fixture_write": startup_write, "matching_frame": startup_frame,
                                     "geometry": startup_geometry,
                                     "launch_to_pty_ready_ns": ready["ready_ns"] - launch_ns,
                                     "launch_to_matching_capture_ns": startup_frame["ready_ns"] - launch_ns,
                                     "initial_grid_matches_requested": startup_geometry_matches(startup_write, startup_geometry, args.cols, args.rows)}
                if startup_write["rows"] != startup_geometry["rows"]:
                    result["startup"]["qualified"] = False
                    result["startup"]["qualification_failure"] = "fixture row count conflicts with observed startup marker geometry"
                else:
                    result["startup"]["qualified"] = result["startup"]["initial_grid_matches_requested"]
                result["running_executable"] = str(Path(f"/proc/{app.pid}/exe").resolve())
                result["running_executable_sha256"] = source_hash(f"/proc/{app.pid}/exe")
                def samples():
                    return {name: process_sample(pid) for name, pid in [("application", app.pid), ("fixture", ready["pid"]),
                                                                      ("compositor", compositor.pid), ("observer", observer.pid)]}
                window = wait("mapped app", lambda: next((n for n in nodes(sway(["-t", "get_tree"])) if n.get("app_id") == app_id), None))
                criteria = f"[con_id={window['id']}]"
                sway([f"{criteria} focus"])
                width, height = args.window_width, args.window_height
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
                result.update(grid=size, window_pixels=[width, height], geometry=geometry, compositor_outputs=sway(["-t", "get_outputs"]))
                result["physical_geometry_qualified"] = bool(args.cell_width and args.cell_height and
                    (geometry["cell_width"], geometry["cell_height"], geometry["rows"]) == (args.cell_width, args.cell_height, args.rows))
                if args.cell_width and args.cell_height and not result["physical_geometry_qualified"]:
                    raise RuntimeError("physical cell geometry does not match the required comparison")

                result["observer_controls"] = []
                for active in (False, True, False, True):
                    observe(f"start {args.nonce} {args.capture_us}" if active else "stop")
                    time.sleep(.15)  # Drain the single outstanding capture before sampling.
                    before = samples()
                    start = time.monotonic_ns(); time.sleep(args.control_seconds); end = time.monotonic_ns()
                    after = samples()
                    captures = [e for e in observations.events if e["event"] == "capture" and start <= e["ready_ns"] <= end]
                    result["observer_controls"].append({"active": active, "start_ns": start, "end_ns": end, "before": before, "after": after,
                        "captures": len(captures), "valid_captures": sum(e["valid"] for e in captures),
                        "decode_wall_ns": sum(e["decode_done_ns"] - e["ready_ns"] for e in captures)})

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
                    keyboard.append({"sequence": sequence, "injected": injected, "fixture": written, "frame": seen,
                        "inject_to_fixture_read_ns": written["read_ns"] - injected["inject_ns"],
                        "fixture_write_to_capture_ns": seen["ready_ns"] - written["write_end_ns"],
                        "inject_to_capture_ns": seen["ready_ns"] - injected["inject_ns"]})
                result["keyboard"] = keyboard
                latencies = [e["inject_to_capture_ns"] for e in keyboard]
                result["keyboard_summary_ns"] = {name: percentile(latencies, q) for name, q in [("p50", .5), ("p95", .95), ("p99", .99), ("max", 1)]}
                result["streams"] = []
                for index, (kind, hz) in enumerate([(kind, hz) for kind in ("marker", "sparse", "scroll") for hz in (30, 60, 0)]):
                    first = 10000 + index * 1000; last = first + args.frames - 1
                    before = samples()
                    send(stream, {"operation": "stream", "start": first, "count": args.frames, "kind": kind, "hz": hz})
                    fixtures.until(lambda e: e["event"] == "stream_done")
                    observations.until(lambda e: e["event"] == "capture" and e["valid"] and e["sequence"] == last)
                    after = samples()
                    writes = [e for e in fixtures.events if e["event"] == "write" and first <= e["sequence"] <= last]
                    seen = [e for e in observations.events if e["event"] == "capture" and e["valid"] and first <= e["sequence"] <= last]
                    if any(a["sequence"] > b["sequence"] for a, b in zip(seen, seen[1:])):
                        raise RuntimeError("observed complete frame sequence went backwards")
                    unique = {}
                    for event in seen:
                        unique.setdefault(event["sequence"], event)
                    result["streams"].append({"kind": kind, "producer_hz": hz, "produced": len(writes), "observed_unique": len(unique),
                        "unobserved_or_coalesced": args.frames - len(unique), "sequences": sorted(unique), "first_write_ns": writes[0]["write_start_ns"],
                        "last_write_ns": writes[-1]["write_end_ns"], "last_capture_ns": unique[last]["ready_ns"],
                        "final_write_to_capture_ns": unique[last]["ready_ns"] - writes[-1]["write_end_ns"],
                        "observed_unique_per_second": len(unique) / ((unique[last]["ready_ns"] - writes[0]["write_start_ns"]) / 1e9),
                        "bytes": sum(e["bytes"] for e in writes), "before": before, "after": after})
                subprocess.run(["grim", str(evidence / "final.png")], env=env, check=True, timeout=5)
                result["status"] = "passed"
    except Exception as error:
        result.update(status="failed", error=repr(error))
        raise
    finally:
        for process in reversed(processes):
            if process.poll() is None:
                with contextlib.suppress(ProcessLookupError): os.killpg(process.pid, signal.SIGTERM)
                try: process.wait(timeout=3)
                except subprocess.TimeoutExpired:
                    with contextlib.suppress(ProcessLookupError): os.killpg(process.pid, signal.SIGKILL)
                    process.wait(timeout=3)
        for reader in readers:
            reader.thread.join(timeout=2); reader.file.close()
        for log in logs: log.close()
        (evidence / "result.json").write_text(json.dumps(result, indent=2) + "\n")
        temporary.cleanup()
    print(json.dumps({"status": result["status"], "geometry_qualified": result["physical_geometry_qualified"], "output": str(evidence)}))


def main():
    if len(sys.argv) == 4 and sys.argv[1] == "--fixture":
        fixture(sys.argv[2], int(sys.argv[3])); return
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("kind", choices=("hotaru", "ghostty")); parser.add_argument("binary", type=Path)
    parser.add_argument("--purpose", choices=("qualification", "measurement"), default="qualification")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--font-dir", type=Path, required=True); parser.add_argument("--fallback-dir", type=Path, required=True)
    parser.add_argument("--mesa", type=Path)
    parser.add_argument("--cols", type=int, default=120); parser.add_argument("--rows", type=int, default=40)
    parser.add_argument("--font-size", type=float, default=14.0); parser.add_argument("--ghostty-cell-height")
    parser.add_argument("--window-width", type=int, default=1400); parser.add_argument("--window-height", type=int, default=1080)
    parser.add_argument("--cell-width", type=int); parser.add_argument("--cell-height", type=int)
    parser.add_argument("--keys", type=int, default=32); parser.add_argument("--frames", type=int, default=60)
    parser.add_argument("--capture-us", type=int, default=1000); parser.add_argument("--control-seconds", type=float, default=3)
    parser.add_argument("--no-pixel-scroll", choices=("0", "1"), default="0", help="explicit Hotaru pixel-copy control, recorded after environment sanitization")
    parser.add_argument("--nonce", type=lambda value: int(value, 0), default=0x29312982)
    parser.add_argument("--app-cpus", default="8-11"); parser.add_argument("--compositor-cpus", default="12-13"); parser.add_argument("--observer-cpus", default="14")
    parser.add_argument("--controller-cpus", default="15"); parser.add_argument("--fixture-cpus", default="16")
    parser.add_argument("--lock", type=Path, default=Path.home() / ".cache/hotaru-workstation.lock")
    args = parser.parse_args()
    if not (76 <= args.cols <= 150 and 4 <= args.rows <= 50 and 1 <= args.keys <= 1000 and 1 <= args.frames <= 500 and
            1000 <= args.capture_us <= 1000000 and .1 <= args.control_seconds <= 60 and 0 <= args.nonce <= 0xffffffff):
        parser.error("argument outside bounded benchmark limits")
    for name in ("binary", "font_dir", "fallback_dir", "mesa"):
        if getattr(args, name) is not None:
            setattr(args, name, getattr(args, name).resolve(strict=True))
    cpu_masks = [cpu_set(getattr(args, name + "_cpus")) for name in ("app", "compositor", "observer", "controller", "fixture")]
    available = os.sched_getaffinity(0)
    if any(not mask <= available for mask in cpu_masks) or sum(map(len, cpu_masks)) != len(set.union(*cpu_masks)):
        parser.error("benchmark CPU sets must be available and disjoint")
    os.sched_setaffinity(0, cpu_set(args.controller_cpus))
    with args.lock.open("a") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        run(args)


if __name__ == "__main__":
    main()
