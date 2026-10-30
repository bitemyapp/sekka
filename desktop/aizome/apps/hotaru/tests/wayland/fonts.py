#!/usr/bin/env python3
"""Physical font-grid/PTY geometry smoke in a private headless Sway session.

Usage: fonts.py BINARY EVIDENCE_DIRECTORY
Requires sway, swaymsg and grim. Font fixture tests separately assert glyph
identities and color pixels; this checks the actual native scale event path.
"""
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import time

from smoke import wait


def run(binary, evidence):
    evidence.mkdir(parents=True, exist_ok=True)
    runtime = Path(tempfile.mkdtemp(prefix="hotaru-fonts-wayland-"))
    os.chmod(runtime, 0o700)
    env = dict(os.environ)
    for key in ["DISPLAY", "WAYLAND_DISPLAY", "SWAYSOCK", "AIZOME_SHELL_SOCKET", "AIZOME_SOCKET", "SIDESTEP_BACKEND"]:
        env.pop(key, None)
    env.update(XDG_RUNTIME_DIR=str(runtime), XDG_CONFIG_HOME=str(runtime / "config"),
               WLR_BACKENDS="headless", WLR_RENDERER="pixman", WLR_LIBINPUT_NO_DEVICES="1",
               XDG_CURRENT_DESKTOP="sway", XDG_SESSION_TYPE="wayland",
               RUST_LOG="warn,aizome_kit::fonts=debug,aizome_hotaru=debug")
    config = runtime / "sway.conf"
    config.write_text("output HEADLESS-1 mode 1280x800\nseat seat0 fallback true\n")
    fixture = runtime / "fixture.py"
    fixture.write_text("""import fcntl, json, os, pathlib, struct, sys, termios, time
path = pathlib.Path(sys.argv[1])
os.write(1, '\\x1b[?25l日本語  😀 👍🏽 👩\\u200d💻 🇯🇵 ❤\\ufe0f ❤\\ufe0e a\\u0301M\\r\\nGrid positions and physical PTY dimensions'.encode())
while True:
    size = struct.unpack('HHHH', fcntl.ioctl(0, termios.TIOCGWINSZ, bytes(8)))
    tmp = path.with_suffix('.tmp')
    tmp.write_text(json.dumps(size))
    tmp.replace(path)
    time.sleep(.02)
""")
    processes = []
    files = []
    result = {"binary": str(binary), "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
              "backend": "private headless Sway, pixman", "scales": [], "fonts": {}}
    try:
        for name, args in [("sway", ["sway", "--unsupported-gpu", "-c", str(config)])]:
            log = open(evidence / f"{name}.log", "wb"); files.append(log)
            processes.append(subprocess.Popen(args, env=env, stdout=log, stderr=subprocess.STDOUT))
        display = wait("private Wayland socket", lambda: next((p for p in runtime.glob("wayland-*") if p.is_socket()), None))
        socket = wait("private Sway socket", lambda: next(runtime.glob("sway-ipc.*.sock"), None))
        env.update(WAYLAND_DISPLAY=display.name, SWAYSOCK=str(socket))
        log = open(evidence / "hotaru.log", "wb"); files.append(log)
        processes.append(subprocess.Popen([str(binary), "--", sys.executable, str(fixture), str(evidence / "pty.json")],
                                          env=env, stdout=log, stderr=subprocess.STDOUT))

        def read_metrics(scale):
            log_text = (evidence / "hotaru.log").read_text(errors="replace")
            matches = re.findall(r'terminal font "([^"]+)", scale ([0-9.]+), grid ([0-9]+)x([0-9]+) px', log_text)
            if not matches or float(matches[-1][1]) != scale:
                return None
            try:
                rows, cols, width, height = json.loads((evidence / "pty.json").read_text())
            except (FileNotFoundError, json.JSONDecodeError):
                return None
            family, _, cw, ch = matches[-1]
            if width != cols * int(cw) or height != rows * int(ch) or not width or not height:
                return None
            return {"scale": scale, "family": family, "cell_width_px": int(cw), "cell_height_px": int(ch),
                    "rows": rows, "cols": cols, "pty_width_px": width, "pty_height_px": height}

        for index, scale in enumerate([1.0, 1.25, 2.0, 1.0]):
            subprocess.run(["swaymsg", "-s", str(socket), f"output HEADLESS-1 scale {scale}"], env=env,
                           stdout=subprocess.DEVNULL, check=True, timeout=3)
            # Initial window/configure and asynchronous font metrics can each
            # resize once. Wait for a consistent pair to remain stable.
            last = [None, time.monotonic()]
            def settled():
                value = read_metrics(scale)
                if value != last[0]:
                    last[:] = [value, time.monotonic()]
                return value if value and time.monotonic() - last[1] >= .2 else None
            metrics = wait(f"font and PTY geometry at scale {scale}", settled)
            subprocess.run(["grim", str(evidence / f"scale-{index}-{scale}.png")], env=env, check=True, timeout=3)
            result["scales"].append(metrics)
        assert result["scales"][0] == result["scales"][-1]
        log_text = (evidence / "hotaru.log").read_text(errors="replace")
        for path in re.findall(r'terminal fallback families .* from (.+)', log_text):
            source = Path(path)
            result["fonts"][source.name] = {"path": str(source), "sha256": hashlib.sha256(source.read_bytes()).hexdigest()}
        assert set(result["fonts"]) == {"NotoColorEmoji.ttf", "NotoSansCJK-Regular.ttc"}
        result["status"] = "passed"
    except Exception as error:
        result.update(status="failed", error=repr(error))
        raise
    finally:
        for process in reversed(processes):
            if process.poll() is None:
                process.terminate()
                try: process.wait(timeout=3)
                except subprocess.TimeoutExpired: process.kill(); process.wait(timeout=3)
        for file in files: file.close()
        (evidence / "result.json").write_text(json.dumps(result, indent=2) + "\n")
        shutil.rmtree(runtime)
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    run(Path(sys.argv[1]).resolve(), Path(sys.argv[2]).resolve())
