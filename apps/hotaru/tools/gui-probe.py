#!/usr/bin/env python3
"""Probe a packaged terminal in a private software-rendered Wayland session.

This records process-to-PTY startup, idle CPU/RSS and PTY-to-DSR turnaround.
DSR acknowledges parsing, not frame presentation. Software rendering, unequal
feature coverage and the deliberately small workload prevent a parity claim.
"""
import argparse
import contextlib
import datetime
import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import select
import signal
import socket
import subprocess
import sys
import tempfile
import time
import tty
from xml.sax.saxutils import escape


def send(stream, value):
    stream.write(json.dumps(value).encode() + b"\n")
    stream.flush()


def receive(stream):
    data = stream.readline(65536)
    if not data.endswith(b"\n"):
        raise RuntimeError("missing or oversized fixture message")
    return json.loads(data)


def write_all(data):
    while data:
        count = os.write(1, data[:65536])
        data = data[count:]


def fixture(address):
    tty.setraw(0)
    with socket.socket(socket.AF_UNIX) as client:
        client.connect(address)
        stream = client.makefile("rwb")
        send(stream, {"ready_ns": time.monotonic_ns(), "pid": os.getpid()})
        while True:
            request = receive(stream)
            if request["operation"] == "size":
                cols, rows = os.get_terminal_size(0)
                send(stream, {"cols": cols, "rows": rows})
            elif request["operation"] == "output":
                data = Path(request["path"]).read_bytes()
                # Preparation happens before the measured write. The final DSR
                # is ordered after all preceding output in the same PTY stream.
                while select.select([0], [], [], 0)[0]:
                    os.read(0, 4096)
                started = time.monotonic_ns()
                write_all(b"\x1b[?25l\x1b[0m\x1b[2J\x1b[H")
                write_all(data)
                write_all(b"\x1b[0m\x1b[HGUI_PROBE_OK\x1b[2;1H\x1b[6n")
                response = bytearray()
                deadline = time.monotonic() + 30
                while time.monotonic() < deadline:
                    if select.select([0], [], [], .1)[0]:
                        response.extend(os.read(0, 4096))
                        if b"\x1b[2;1R" in response:
                            break
                    if len(response) > 65536:
                        raise RuntimeError("unexpected terminal response flood")
                ended = time.monotonic_ns()
                if bytes(response) != b"\x1b[2;1R":
                    raise RuntimeError(f"unexpected DSR response: {response!r}")
                send(stream, {"write_to_dsr_ns": ended - started,
                              "response_hex": response.hex(), "bytes": len(data)})
            else:
                raise RuntimeError("unknown fixture operation")


def wait(label, predicate, timeout=30):
    until = time.monotonic() + timeout
    while time.monotonic() < until:
        value = predicate()
        if value:
            return value
        time.sleep(.02)
    raise RuntimeError(f"timed out: {label}")


def nodes(node):
    yield node
    for child in node.get("nodes", []) + node.get("floating_nodes", []):
        yield from nodes(child)


def process_sample(pid):
    # comm may contain spaces and parentheses. Fields following its closing
    # parenthesis start at stat field 3; utime/stime are fields 14 and 15.
    stat = Path(f"/proc/{pid}/stat").read_text().rsplit(")", 1)[1].split()
    status = Path(f"/proc/{pid}/status").read_text()
    memory = {}
    for key in ("VmRSS", "VmHWM", "RssAnon", "RssFile", "Threads"):
        match = re.search(rf"^{key}:\s+(\d+)", status, re.MULTILINE)
        memory[key] = int(match[1]) if match else None
    return {"sample_ns": time.monotonic_ns(), "cpu_ticks": int(stat[11]) + int(stat[12]),
            "memory_kib_except_threads": memory}


def probe(args):
    binary = args.binary.absolute()
    evidence = args.output.absolute()
    evidence.mkdir(parents=True, exist_ok=False)
    temporary = tempfile.TemporaryDirectory(prefix="hotaru-gui-probe-")
    runtime = Path(temporary.name)
    os.chmod(runtime, 0o700)
    processes = []
    logs = []
    env = dict(os.environ)
    for name in ("DISPLAY", "WAYLAND_DISPLAY", "SWAYSOCK", "DBUS_SESSION_BUS_ADDRESS",
                 "AIZOME_SHELL_SOCKET", "AIZOME_SOCKET", "AIZOME_KIT_INSPECT", "SIDESTEP_BACKEND",
                 "AIZOME_FONT_DIR", "FONTCONFIG_FILE", "FONTCONFIG_PATH", "LD_LIBRARY_PATH"):
        env.pop(name, None)
    for directory in ("home", "config", "cache", "data"):
        (runtime / directory).mkdir()
    env.update(HOME=str(runtime / "home"), XDG_RUNTIME_DIR=str(runtime),
               XDG_CONFIG_HOME=str(runtime / "config"), XDG_CACHE_HOME=str(runtime / "cache"),
               XDG_DATA_HOME=str(runtime / "data"), XDG_CURRENT_DESKTOP="sway",
               XDG_SESSION_TYPE="wayland", WLR_BACKENDS="headless", WLR_RENDERER="pixman",
               WLR_LIBINPUT_NO_DEVICES="1", LIBGL_ALWAYS_SOFTWARE="1", GALLIUM_DRIVER="llvmpipe")
    font_config = runtime / "fonts.conf"
    font_directory = args.font_dir.resolve(strict=True)
    font_config.write_text('<?xml version="1.0"?><!DOCTYPE fontconfig SYSTEM "fonts.dtd">'
                           '<fontconfig><dir>' + escape(str(font_directory)) + '</dir>'
                           '<cachedir>' + escape(str(runtime / "cache/fonts")) + '</cachedir></fontconfig>')
    env.update(FONTCONFIG_FILE=str(font_config), AIZOME_FONT_DIR=str(font_directory))
    if args.mesa:
        mesa = args.mesa.resolve(strict=True)
        env.update(__EGL_VENDOR_LIBRARY_FILENAMES=str(mesa / "share/glvnd/egl_vendor.d/50_mesa.json"),
                   LIBGL_DRIVERS_PATH=str(mesa / "lib/dri"))
    config = runtime / "sway.conf"
    config.write_text("output HEADLESS-1 mode 1920x1440\nseat seat0 fallback true\n"
                      "focus_follows_mouse no\ndefault_border none\n")

    def launch(name, command, **kwargs):
        log = (evidence / f"{name}.log").open("wb")
        logs.append(log)
        process = subprocess.Popen(command, env=env, stdout=log, stderr=subprocess.STDOUT,
                                   start_new_session=True, **kwargs)
        processes.append(process)
        return process

    try:
        dbus_address = "unix:path=" + str(runtime / "dbus")
        # No service directories: graphical probes must not activate the host's
        # portal daemons or leave document-portal FUSE mounts after teardown.
        bus_config = runtime / "dbus.conf"
        bus_config.write_text('<busconfig><type>session</type><listen>' + escape(dbus_address) + '</listen>'
                              '<auth>EXTERNAL</auth><policy context="default">'
                              '<allow send_destination="*"/><allow receive_sender="*"/>'
                              '<allow own="*"/></policy></busconfig>')
        launch("dbus", ["dbus-daemon", "--nofork", "--nopidfile", "--config-file=" + str(bus_config)])
        wait("private D-Bus", lambda: (runtime / "dbus").is_socket())
        env["DBUS_SESSION_BUS_ADDRESS"] = dbus_address
        launch("sway", ["sway", "--unsupported-gpu", "-c", str(config)])
        display = wait("private Wayland", lambda: next(
            (p for p in runtime.glob("wayland-*") if p.is_socket()), None))
        sway_socket = wait("private Sway", lambda: next(runtime.glob("sway-ipc.*.sock"), None))
        env.update(WAYLAND_DISPLAY=display.name, SWAYSOCK=str(sway_socket))

        def sway(*command):
            return json.loads(subprocess.check_output(["swaymsg", "-s", str(sway_socket), *command],
                                                      env=env, timeout=3))

        address = str(runtime / "control")
        with socket.socket(socket.AF_UNIX) as listener:
            listener.bind(address)
            listener.listen(1)
            listener.settimeout(45)
            child = [sys.executable, str(Path(__file__).resolve()), "--fixture", address]
            if args.kind == "ghostty":
                config_text = ("gtk-single-instance = false\nshell-integration = none\n"
                               "scrollback-limit-bytes = unlimited\nscrollback-limit-lines = 0\n"
                               "font-family = IBM Plex Mono\nfont-size = 14\ncursor-style-blink = false\n")
                config_path = runtime / "ghostty.conf"
                config_path.write_text(config_text)
                command = [str(binary), "--config-default-files=false", "--config-file=" + str(config_path),
                           "-e", *child]
                app_id = "com.mitchellh.ghostty"
            else:
                config_text = 'font_size = 14.0\nscrollback = 0\ntheme = "night"\n'
                config_path = runtime / "hotaru.toml"
                config_path.write_text(config_text)
                command = [str(binary), "--config-file", str(config_path), "--", *child]
                app_id = "org.aizome.Hotaru"
            started = time.monotonic_ns()
            app = launch(args.kind, command)
            connection, _ = listener.accept()
            with connection:
                connection.settimeout(45)
                stream = connection.makefile("rwb")
                ready = receive(stream)
                window = wait("mapped application", lambda: next(
                    (n for n in nodes(sway("-t", "get_tree")) if n.get("app_id") == app_id), None))
                mapped_ns = time.monotonic_ns()
                criteria = f"[con_id={window['id']}]"
                sway(f"{criteria} floating enable, border none, resize set 1100 px 820 px, focus")

                def dimensions():
                    send(stream, {"operation": "size"})
                    return receive(stream)

                # Find the exact cell dimensions empirically, without assuming
                # equal chrome sizes or equal point-to-pixel conversion.
                width, height = 1100, 820
                for _ in range(120):
                    time.sleep(.04)
                    size = dimensions()
                    if size == {"cols": args.cols, "rows": args.rows}:
                        break
                    width += max(-40, min(40, (args.cols - size["cols"]) * 2))
                    height += max(-40, min(40, (args.rows - size["rows"]) * 2))
                    if not 100 <= width <= 1870 or not 100 <= height <= 1390:
                        raise RuntimeError(f"requested grid exceeds private output: {size}, {width}x{height}")
                    sway(f"{criteria} resize set {width} px {height} px")
                else:
                    raise RuntimeError(f"grid calibration did not converge: {size}")
                time.sleep(.5)
                idle_before = process_sample(app.pid)
                time.sleep(args.idle_seconds)
                idle_after = process_sample(app.pid)
                ticks = os.sysconf("SC_CLK_TCK")
                idle_seconds = (idle_after["sample_ns"] - idle_before["sample_ns"]) / 1e9
                result = {
                    "schema": 1, "recorded_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
                    "kind": args.kind, "binary": str(binary),
                    "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
                    "probe_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
                    "running_executable": str(Path(f"/proc/{app.pid}/exe").resolve()),
                    "running_executable_sha256": hashlib.sha256(Path(f"/proc/{app.pid}/exe").read_bytes()).hexdigest(),
                    "command": command, "configuration": config_text,
                    "font_directory": str(font_directory), "fontconfig": font_config.read_text(),
                    "mesa_directory": str(args.mesa.resolve()) if args.mesa else None,
                    "backend": "private Sway pixman; LIBGL_ALWAYS_SOFTWARE=1, GALLIUM_DRIVER=llvmpipe",
                    "bus": "private session D-Bus without service activation",
                    "scope": "Exploratory software GUI probe; DSR is parse completion, not presentation",
                    "grid": size, "window_pixels": [width, height],
                    "launch_to_pty_ready_ms": (ready["ready_ns"] - started) / 1e6,
                    "launch_to_observed_window_ms": (mapped_ns - started) / 1e6,
                    "idle_seconds": idle_seconds,
                    "idle_cpu_seconds": (idle_after["cpu_ticks"] - idle_before["cpu_ticks"]) / ticks,
                    "idle_process": idle_after,
                    "host": os.uname().nodename, "kernel": os.uname().release,
                    "cpu_affinity": sorted(os.sched_getaffinity(0)),
                    "load_average": os.getloadavg(), "workloads": [],
                }
                for data in args.data:
                    data = data.resolve(strict=True)
                    before = process_sample(app.pid)
                    send(stream, {"operation": "output", "path": str(data)})
                    timing = receive(stream)
                    after = process_sample(app.pid)
                    time.sleep(.3)
                    screenshot = f"{data.stem}.png"
                    subprocess.run(["grim", str(evidence / screenshot)], env=env, check=True, timeout=5)
                    result["workloads"].append({"path": str(data), "sha256": hashlib.sha256(data.read_bytes()).hexdigest(),
                                                **timing, "process_before": before, "process_after": after,
                                                "screenshot": screenshot})
                (evidence / "result.json").write_text(json.dumps(result, indent=2) + "\n")
                print(json.dumps({"kind": args.kind, "grid": size,
                                  "pty_ready_ms": result["launch_to_pty_ready_ms"],
                                  "idle_cpu_seconds": result["idle_cpu_seconds"],
                                  "rss_kib": idle_after["memory_kib_except_threads"]["VmRSS"],
                                  "output": str(evidence)}))
    finally:
        for process in reversed(processes):
            if process.poll() is None:
                with contextlib.suppress(ProcessLookupError):
                    os.killpg(process.pid, signal.SIGTERM)
                try:
                    process.wait(timeout=3)
                except subprocess.TimeoutExpired:
                    with contextlib.suppress(ProcessLookupError):
                        os.killpg(process.pid, signal.SIGKILL)
                    process.wait(timeout=3)
        for log in logs:
            log.close()
        temporary.cleanup()


def main():
    if len(sys.argv) == 3 and sys.argv[1] == "--fixture":
        fixture(sys.argv[2])
        return
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("kind", choices=("hotaru", "ghostty"))
    parser.add_argument("binary", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--data", type=Path, action="append", default=[])
    parser.add_argument("--font-dir", type=Path, required=True,
                        help="shared Aizome font bundle used by both applications")
    parser.add_argument("--mesa", type=Path, help="Mesa matching the Nix GL application's libc")
    parser.add_argument("--cols", type=int, default=120)
    parser.add_argument("--rows", type=int, default=40)
    parser.add_argument("--idle-seconds", type=float, default=3)
    parser.add_argument("--lock", type=Path, default=Path.home() / ".cache/hotaru-workstation.lock")
    args = parser.parse_args()
    if not 20 <= args.cols <= 150 or not 5 <= args.rows <= 45 or not .1 <= args.idle_seconds <= 60:
        parser.error("grid or idle duration outside bounded probe limits")
    args.lock.parent.mkdir(parents=True, exist_ok=True)
    with args.lock.open("a") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        probe(args)


if __name__ == "__main__":
    main()
