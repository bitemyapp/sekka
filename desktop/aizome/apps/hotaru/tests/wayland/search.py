#!/usr/bin/env python3
"""Native Find shortcuts, history navigation, counts and match-copy smoke.

Usage: search.py BINARY EVIDENCE_DIRECTORY
Uses a private disposable headless Sway, never the user's session.
"""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time

from smoke import wait


def run(binary, evidence):
    evidence.mkdir(parents=True, exist_ok=True)
    build = evidence / "keyboard"
    build.mkdir(exist_ok=True)
    xml = next(Path.home().glob(".cargo/registry/src/*/wayland-protocols-misc-*/protocols/virtual-keyboard-unstable-v1.xml"))
    subprocess.run(["wayland-scanner", "client-header", str(xml), str(build / "virtual-keyboard-client.h")], check=True)
    subprocess.run(["wayland-scanner", "private-code", str(xml), str(build / "virtual-keyboard.c")], check=True)
    flags = subprocess.check_output(["pkg-config", "--cflags", "--libs", "wayland-client", "xkbcommon"], text=True).split()
    subprocess.run(["cc", "-O2", "-Wall", "-Wextra", "-Werror", "-I" + str(build), str(Path(__file__).with_name("key.c")),
                    str(build / "virtual-keyboard.c"), "-o", str(build / "key"), *flags], check=True)
    runtime = Path(tempfile.mkdtemp(prefix="hotaru-search-wayland-"))
    os.chmod(runtime, 0o700)
    env = dict(os.environ)
    for name in ["DISPLAY", "WAYLAND_DISPLAY", "SWAYSOCK", "AIZOME_SHELL_SOCKET", "AIZOME_SOCKET", "SIDESTEP_BACKEND"]:
        env.pop(name, None)
    inspect = evidence / "inspect"
    inspect.mkdir(exist_ok=True)
    env.update(XDG_RUNTIME_DIR=str(runtime), XDG_CONFIG_HOME=str(runtime / "config"), WLR_BACKENDS="headless",
               WLR_RENDERER="pixman", WLR_LIBINPUT_NO_DEVICES="1", XDG_CURRENT_DESKTOP="sway", XDG_SESSION_TYPE="wayland",
               AIZOME_KIT_INSPECT=str(inspect))
    config = runtime / "sway.conf"
    config.write_text("output HEADLESS-1 mode 1280x800\nseat seat0 fallback true\n")
    fixture = runtime / "fixture.py"
    fixture.write_text("""import os, pathlib, sys, time
root = pathlib.Path(sys.argv[1])
while not (root / 'produce').exists(): time.sleep(.01)
os.write(1, ''.join(f'Needle {n:03}\\r\\n' for n in range(120)).encode())
(root / 'ready').touch()
while True: time.sleep(1)
""")
    for name in ["produce", "ready"]: (evidence / name).unlink(missing_ok=True)
    processes, files = [], []
    result = {"binary": str(binary), "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
              "backend": "private headless Sway, pixman", "checks": {}}
    try:
        log = open(evidence / "sway.log", "wb"); files.append(log)
        processes.append(subprocess.Popen(["sway", "--unsupported-gpu", "-c", str(config)], env=env, stdout=log, stderr=subprocess.STDOUT))
        display = wait("private Wayland socket", lambda: next((p for p in runtime.glob("wayland-*") if p.is_socket()), None))
        socket = wait("private Sway socket", lambda: next(runtime.glob("sway-ipc.*.sock"), None))
        env.update(WAYLAND_DISPLAY=display.name, SWAYSOCK=str(socket))
        log = open(evidence / "hotaru.log", "wb"); files.append(log)
        app = subprocess.Popen([str(binary), "--", sys.executable, str(fixture), str(evidence)], env=env, stdout=log, stderr=subprocess.STDOUT)
        processes.append(app)

        def inspected():
            try: return json.loads(next(inspect.glob(f"org.aizome.Hotaru.{app.pid}.*.json")).read_text())
            except (StopIteration, FileNotFoundError, json.JSONDecodeError): return None

        def has_text(text):
            state = inspected()
            return state and any(item.get("text") == text for item in state["elements"])

        def keys(events):
            args = [value for code, modifiers in events for value in [str(code), modifiers]]
            subprocess.run([str(build / "key"), *args], env=env, check=True, timeout=3)

        wait("native window", inspected)
        time.sleep(.3)
        (evidence / "produce").touch()
        wait("fixture output", lambda: (evidence / "ready").exists())
        time.sleep(.3)
        keys([(33, "super")])
        wait("Find field", lambda: has_text(""))
        keys([(49, "none"), (18, "none"), (18, "none"), (32, "none"), (38, "none"), (18, "none")])
        wait("case-insensitive whole-history result count", lambda: has_text("1 of 120"))
        keys([(34, "super")])
        wait("Find Next", lambda: has_text("2 of 120"))
        keys([(34, "super,shift")])
        wait("Find Previous", lambda: has_text("1 of 120"))
        keys([(34, "super,shift")])
        wait("wrapped oldest history match", lambda: has_text("120 of 120"))
        keys([(46, "super")])
        copied = wait("native match copy", lambda: subprocess.run(["wl-paste", "--no-newline"], env=env, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, timeout=3).stdout == b"Needle")
        assert copied
        result["checks"]["history_navigation"] = {"matches": 120, "order": [1, 2, 1, 120], "copied_match": "Needle"}
        subprocess.run(["grim", str(evidence / "oldest-match.png")], env=env, check=True, timeout=3)
        keys([(30, "super"), (49, "none"), (24, "none"), (49, "none"), (18, "none")])
        wait("obsolete query replaced", lambda: has_text("0 matches"))
        result["checks"]["query_replacement"] = {"query": "none", "matches": 0}
        subprocess.run(["grim", str(evidence / "no-matches.png")], env=env, check=True, timeout=3)
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
