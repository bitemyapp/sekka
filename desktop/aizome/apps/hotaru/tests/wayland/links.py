#!/usr/bin/env python3
"""OSC8 activation in a private Wayland session with a disposable fake opener.

Usage: links.py BINARY EVIDENCE_DIRECTORY
The fixture's explicit PATH intercepts xdg-open; no browser or network is used.
"""
import hashlib
import json
import os
from pathlib import Path
import select
import shutil
import subprocess
import sys
import tempfile

from smoke import wait


def run(binary, evidence):
    evidence.mkdir(parents=True, exist_ok=True)
    build = evidence / "input"
    build.mkdir(exist_ok=True)
    specs = [("virtual-keyboard", ".cargo/registry/src/*/wayland-protocols-misc-*/protocols/virtual-keyboard-unstable-v1.xml"),
             ("virtual-pointer", ".cargo/registry/src/*/wayland-protocols-wlr-*/wlr-protocols/unstable/wlr-virtual-pointer-unstable-v1.xml")]
    for name, pattern in specs:
        xml = next(Path.home().glob(pattern))
        subprocess.run(["wayland-scanner", "client-header", str(xml), str(build / f"{name}-client.h")], check=True)
        subprocess.run(["wayland-scanner", "private-code", str(xml), str(build / f"{name}.c")], check=True)
    flags = subprocess.check_output(["pkg-config", "--cflags", "--libs", "wayland-client", "xkbcommon"], text=True).split()
    subprocess.run(["cc", "-O2", "-Wall", "-Wextra", "-Werror", "-I" + str(build),
                    str(Path(__file__).with_name("link-pointer.c")), *[str(build / f"{name}.c") for name, _ in specs],
                    "-o", str(build / "link-pointer"), *flags], check=True)
    runtime = Path(tempfile.mkdtemp(prefix="hotaru-links-wayland-"))
    os.chmod(runtime, 0o700)
    opener = runtime / "bin"
    opener.mkdir()
    calls = evidence / "opener.jsonl"
    calls.unlink(missing_ok=True)
    fake = opener / "xdg-open"
    fake.write_text("#!/usr/bin/python3\nimport json, os, pathlib, sys\n"
                    "with open(os.environ['HOTARU_TEST_OPEN_RECORD'], 'a') as f: f.write(json.dumps(sys.argv[1:]) + '\\n')\n"
                    "sys.exit(3 if pathlib.Path(os.environ['HOTARU_TEST_OPEN_FAIL']).exists() else 0)\n")
    fake.chmod(0o700)
    env = dict(os.environ)
    for name in ["DISPLAY", "WAYLAND_DISPLAY", "SWAYSOCK", "AIZOME_SHELL_SOCKET", "AIZOME_SOCKET", "SIDESTEP_BACKEND"]:
        env.pop(name, None)
    inspect = evidence / "inspect"
    inspect.mkdir(exist_ok=True)
    env.update(XDG_RUNTIME_DIR=str(runtime), XDG_CONFIG_HOME=str(runtime / "config"), WLR_BACKENDS="headless",
               WLR_RENDERER="pixman", WLR_LIBINPUT_NO_DEVICES="1", XDG_CURRENT_DESKTOP="sway", XDG_SESSION_TYPE="wayland",
               AIZOME_KIT_INSPECT=str(inspect), PATH=str(opener) + ":/usr/bin:/bin", HOTARU_TEST_OPEN_RECORD=str(calls),
               HOTARU_TEST_OPEN_FAIL=str(runtime / "fail"))
    config = runtime / "sway.conf"
    config.write_text("output HEADLESS-1 mode 1280x800\nseat seat0 fallback true\n")
    target = "https://example.test/日本?x=$(false)&quote='literal'"
    fixture = runtime / "fixture.py"
    fixture.write_text("""import os, pathlib, sys, time
root = pathlib.Path(sys.argv[1]); previous = None
while True:
    command = (root / 'stage').read_text() if (root / 'stage').exists() else ''
    if command != previous and command:
        target = sys.argv[2].encode() if command != 'invalid' else b'https://example.test/\\xff'
        os.write(1, b'\\x1b[2J\\x1b[H\\x1b]8;id=test;' + target + b'\\x1b\\\\' + '界link'.encode() + b'\\x1b]8;;\\x1b\\\\ plain ' + command.encode() + b'\\x1b]2;' + command.encode() + b'\\x07')
        previous = command
    time.sleep(.01)
""")
    processes, files = [], []
    result = {"binary": str(binary), "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
              "backend": "private headless Sway, pixman", "opener": "disposable fake xdg-open via explicit PATH", "checks": {}}
    try:
        log = open(evidence / "sway.log", "wb"); files.append(log)
        processes.append(subprocess.Popen(["sway", "--unsupported-gpu", "-c", str(config)], env=env, stdout=log, stderr=subprocess.STDOUT))
        display = wait("private Wayland socket", lambda: next((p for p in runtime.glob("wayland-*") if p.is_socket()), None))
        socket = wait("private Sway socket", lambda: next(runtime.glob("sway-ipc.*.sock"), None))
        env.update(WAYLAND_DISPLAY=display.name, SWAYSOCK=str(socket))
        log = open(evidence / "hotaru.log", "wb"); files.append(log)
        app = subprocess.Popen([str(binary), "--", sys.executable, str(fixture), str(runtime), target], env=env, stdout=log, stderr=subprocess.STDOUT)
        processes.append(app)

        def inspected():
            try: return json.loads(next(inspect.glob(f"org.aizome.Hotaru.{app.pid}.*.json")).read_text())
            except (StopIteration, FileNotFoundError, json.JSONDecodeError): return None

        def texts():
            return [item.get("text", "") for item in (inspected() or {}).get("elements", [])]

        def opened():
            try: return [json.loads(line) for line in calls.read_text().splitlines()]
            except FileNotFoundError: return []

        def stage(name):
            (runtime / "stage").write_text(name)
            wait("native stage title", lambda: (inspected() or {}).get("title") == name + " — Hotaru")

        wait("native window", inspected)
        stage("links-ready")
        tree = json.loads(subprocess.check_output(["swaymsg", "-t", "get_tree"], env=env))
        def app_node(node):
            if node.get("pid") == app.pid: return node
            for child in node.get("nodes", []) + node.get("floating_nodes", []):
                found = app_node(child)
                if found: return found
        node = app_node(tree)
        content_x = node["rect"]["x"] + node["window_rect"]["x"]
        content_y = node["rect"]["y"] + node["window_rect"]["y"]
        terminal = next(item for item in inspected()["elements"] if item.get("name", "").startswith("terminal-"))
        x = int(content_x + terminal["rect"][0] + 10 + 18)  # wide continuation cell at the default grid
        y = int(content_y + terminal["rect"][1] + 10 + 10)

        def pointer(mode):
            p = subprocess.Popen([str(build / "link-pointer"), str(x), str(y), mode], env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
            processes.append(p)
            assert select.select([p.stdout], [], [], 3)[0], "pointer fixture did not become ready"
            assert p.stdout.readline().strip() == "ready"
            return p

        def finish(p):
            if p.poll() is None:
                try: p.stdin.write("\n"); p.stdin.flush()
                except BrokenPipeError: pass
            assert p.wait(timeout=3) == 0

        hover = pointer("hover")
        wait("actual OSC8 target exposed in status", lambda: target in texts())
        subprocess.run(["grim", str(evidence / "link-hover.png")], env=env, check=True, timeout=3)
        finish(hover)
        finish(pointer("super"))
        wait("one exact desktop target", lambda: len(opened()) == 1)
        assert opened() == [[target]]
        finish(pointer("plain"))
        finish(pointer("drag"))
        hold = pointer("hold")
        stage("changed-frame")
        # Wait for the actual hover to disappear when the new frame is displayed.
        wait("old displayed link invalidated", lambda: target not in texts())
        finish(hold)
        stage("invalid")
        finish(pointer("super"))
        # A valid final click is an ordering barrier for all earlier cancelled
        # gestures. No sleep is used to conclude that they did not launch.
        stage("valid-again")
        hover = pointer("hover")
        wait("new valid frame displayed", lambda: target in texts())
        finish(hover)
        (runtime / "fail").touch()
        stationary = pointer("stationary")
        wait("failed opener status", lambda: any("Could not open link:" in text and "exit status 3" in text for text in texts()))
        assert stationary.poll() is None, "pointer device must still be present"
        subprocess.run(["grim", str(evidence / "stationary-open-error.png")], env=env, check=True, timeout=3)
        finish(stationary)
        assert opened() == [[target], [target]], opened()
        result["checks"] = {"exact_target": target, "opened_argv": opened(), "wide_tail": True,
                            "plain_click_drag_changed_frame_invalid_utf8_cancelled": True, "failed_opener_reported_with_stationary_pointer": True}
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
