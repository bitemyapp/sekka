#!/usr/bin/env python3
"""Qualify the observer/fixture without benchmarking either application."""
import argparse
import contextlib
import fcntl
import importlib.util
import json
import os
from pathlib import Path
import pty
import signal
import socket
import struct
import subprocess
import sys
import tempfile
import termios
import threading
import time

SPEC = importlib.util.spec_from_file_location("frame_benchmark", Path(__file__).with_name("frame-benchmark.py"))
BENCH = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(BENCH)


def stop(process):
    if process.poll() is None:
        with contextlib.suppress(ProcessLookupError): os.killpg(process.pid, signal.SIGTERM)
        try: process.wait(timeout=3)
        except subprocess.TimeoutExpired:
            with contextlib.suppress(ProcessLookupError): os.killpg(process.pid, signal.SIGKILL)
            process.wait(timeout=3)


def fixture_test(evidence):
    with tempfile.TemporaryDirectory(prefix="hotaru-frame-pty-") as temp:
        control = str(Path(temp) / "control")
        master, slave = pty.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 80, 0, 0))
        captured = bytearray()

        def drain():
            while True:
                try: data = os.read(master, 65536)
                except OSError: break
                if not data: break
                captured.extend(data)

        process = None
        with socket.socket(socket.AF_UNIX) as listener, (evidence / "fixture-stderr.log").open("wb") as log:
            listener.bind(control); listener.listen(1); listener.settimeout(5)
            try:
                process = subprocess.Popen([sys.executable, str(Path(__file__).with_name("frame-benchmark.py")), "--fixture", control, "123"],
                    stdin=slave, stdout=slave, stderr=log, start_new_session=True)
                os.close(slave)
                thread = threading.Thread(target=drain, daemon=True); thread.start()
                connection, _ = listener.accept()
                with connection:
                    stream = connection.makefile("rwb", buffering=0)
                    reader = BENCH.JsonReader(stream, evidence / "fixture-events.jsonl")
                    reader.until(lambda e: e["event"] == "ready")
                    reader.until(lambda e: e["event"] == "write" and e["sequence"] == 0)
                    fcntl.ioctl(master, termios.TIOCSWINSZ, struct.pack("HHHH", 28, 90, 0, 0))
                    os.kill(process.pid, signal.SIGWINCH)
                    reader.until(lambda e: e["event"] == "write" and e["sequence"] == 0 and e["rows"] == 28)
                    BENCH.send(stream, {"operation": "paint", "sequence": 1, "kind": "initial"})
                    reader.until(lambda e: e["event"] == "write" and e["sequence"] == 1)
                    BENCH.send(stream, {"operation": "arm", "start": 100, "count": 3})
                    reader.until(lambda e: e["event"] == "armed")
                    for seq in range(100, 103):
                        os.write(master, b"a")
                        item = reader.until(lambda e: e["event"] == "write" and e["sequence"] == seq)
                        assert item["read_ns"] <= item["write_start_ns"] <= item["write_end_ns"]
                    for start, kind in [(1000, "sparse"), (2000, "scroll"), (3000, "marker")]:
                        BENCH.send(stream, {"operation": "stream", "start": start, "count": 8, "kind": kind, "hz": 0})
                        reader.until(lambda e: e["event"] == "stream_done")
                        writes = [e for e in reader.events if e["event"] == "write" and start <= e["sequence"] < start + 8]
                        assert [e["sequence"] for e in writes] == list(range(start, start + 8))
                    BENCH.send(stream, {"operation": "quit"})
                    assert process.wait(timeout=3) == 0
                    reader.thread.join(1); reader.file.close(); thread.join(1)
                    for event in reader.events:
                        if event["event"] == "write":
                            expected = BENCH.output_bytes(event["sequence"], 123, event["cols"], event["rows"], event["kind"])
                            assert expected in captured
                            assert BENCH.crc(event["sequence"], 123) == event["crc"]
                    return {"status": "passed", "writes": sum(e["event"] == "write" for e in reader.events),
                            "startup_resize_repaint": True, "real_pty_key_read": True, "captured_bytes": len(captured)}
            finally:
                if process: stop(process)
                os.close(master)


def protocol_test(evidence, binary):
    with tempfile.TemporaryDirectory(prefix="hotaru-observer-smoke-") as temp:
        runtime = Path(temp); os.chmod(runtime, 0o700)
        env = dict(os.environ)
        for key in ["DISPLAY", "WAYLAND_DISPLAY", "SWAYSOCK", "DBUS_SESSION_BUS_ADDRESS"]: env.pop(key, None)
        env.update(XDG_RUNTIME_DIR=temp, XDG_CONFIG_HOME=temp, WLR_BACKENDS="headless", WLR_RENDERER="pixman", WLR_LIBINPUT_NO_DEVICES="1")
        config = runtime / "sway.conf"; config.write_text("output HEADLESS-1 mode 640x480\nseat seat0 fallback true\n")
        logs = [(evidence / "protocol-sway.log").open("wb"), (evidence / "protocol-observer.log").open("wb")]
        processes = []
        try:
            sway = subprocess.Popen(["sway", "--unsupported-gpu", "-c", str(config)], env=env, stdout=logs[0], stderr=subprocess.STDOUT, start_new_session=True)
            processes.append(sway)
            display = BENCH.wait("Wayland", lambda: next((p for p in runtime.glob("wayland-*") if p.is_socket()), None))
            env["WAYLAND_DISPLAY"] = display.name
            observer = subprocess.Popen([str(binary)], env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=logs[1], bufsize=0, start_new_session=True)
            processes.append(observer)
            reader = BENCH.JsonReader(observer.stdout, evidence / "protocol-events.jsonl")
            reader.until(lambda e: e["event"] == "ready")

            def command(value):
                observer.stdin.write((value + "\n").encode()); observer.stdin.flush()

            command("start 123 16000")
            for _ in range(4): reader.until(lambda e: e["event"] == "capture")
            command("stop"); time.sleep(.1)
            stopped = time.monotonic_ns(); time.sleep(.1)
            assert not any(e["event"] == "capture" and e["ready_ns"] > stopped for e in reader.events)
            command("key 42 30")
            reader.until(lambda e: e["event"] == "key" and e["sequence"] == 42)
            command("start 123 16000")
            for _ in range(3): reader.until(lambda e: e["event"] == "capture" and e["ready_ns"] > stopped)
            command("quit"); assert observer.wait(timeout=3) == 0
            reader.thread.join(1); reader.file.close()
            return {"status": "passed", "scope": "empty private 640x480 pixman compositor, no application benchmark",
                    "captures": sum(e["event"] == "capture" for e in reader.events), "start_stop_restart": True,
                    "virtual_keyboard_transport": True, "no_host_configuration_changes": True}
        finally:
            for process in reversed(processes): stop(process)
            for log in logs: log.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    parser.add_argument("--lock", type=Path, default=Path.home() / ".cache/hotaru-workstation.lock")
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    with args.lock.open("a") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        binary = BENCH.build_observer(args.output / "build")
        build = json.loads((args.output / "build/build.json").read_text())
        command = build["command"]
        original = str(Path(__file__).with_name("frame-observer.c"))
        command[command.index(original)] = str(Path(__file__).with_name("frame-observer-test.c"))
        command[-1] = str(args.output / "build/observer-test")
        subprocess.run(command, check=True)
        output = subprocess.check_output([command[-1]], text=True)
        assert f"checksum={BENCH.crc(0x12345678, 0x29312982):08x}" in output
        (args.output / "decoder-test.log").write_text(output)
        for cols in (119, 120):
            for fixture_rows in (39, 40):
                for observed_rows in (39, 40):
                    got = BENCH.startup_geometry_matches({"cols": cols, "rows": fixture_rows}, {"rows": observed_rows}, 120, 40)
                    assert got == (cols == 120 and fixture_rows == observed_rows == 40)
        result = {"status": "passed", "decoder": output.splitlines()[-1], "fixture": fixture_test(args.output),
                  "protocol": protocol_test(args.output, binary),
                  "startup_geometry_cases": 8,
                  "sources": {name: BENCH.source_hash(Path(__file__).with_name(name)) for name in
                              ["frame-benchmark.py", "frame-observer.c", "frame-observer-test.c", "frame-harness-check.py"]}}
        (args.output / "result.json").write_text(json.dumps(result, indent=2) + "\n")
        print(json.dumps(result))


if __name__ == "__main__":
    main()
