#!/usr/bin/env python3
"""Real native clipboard smoke in an isolated, disposable headless Sway session.

Usage: smoke.py /absolute/path/to/aizome-hotaru /absolute/evidence/directory
Requires sway, swaymsg, wl-copy, wl-paste, grim, cc, pkg-config and wayland-scanner.
The virtual-keyboard XML comes from an existing Cargo registry checkout.
"""
import hashlib
import json
import os
from datetime import datetime, timezone
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time


def wait(label, predicate, timeout=15):
    until = time.monotonic() + timeout
    while time.monotonic() < until:
        value = predicate()
        if value:
            return value
        time.sleep(0.02)
    raise AssertionError(f"Timed out: {label}")


def run(binary, evidence):
    evidence.mkdir(parents=True, exist_ok=True)
    root = Path(__file__).resolve().parents[6]
    source_paths = set()
    for directory in ["desktop/aizome/apps/hotaru", "apps/hotaru/crates/hotaru-core", "apps/hotaru/crates/hotaru-io"]:
        source_paths.update(path for path in (root / directory).rglob("*")
                            if path.is_file() and path.suffix in {".rs", ".c", ".py", ".toml"})
    source_paths.update(root / path for path in [
        "desktop/aizome/Cargo.toml", "desktop/aizome/Cargo.lock", "apps/hotaru/Cargo.toml", "apps/hotaru/Cargo.lock",
        *[f"desktop/aizome/crates/aizome-kit/src/{name}.rs" for name in ["app", "cx", "lib", "platform", "testing", "engine", "menu"]],
        "vendor/sidestep/crates/sidestep-engine/src/clipboard.rs",
        "vendor/sidestep/crates/sidestep-engine/src/backend/selection.rs",
        "vendor/sidestep/crates/sidestep-ui/src/app.rs", "vendor/sidestep/crates/sidestep-ui/src/lib.rs",
        "vendor/sidestep/crates/sidestep-engine/src/protocol.rs",
        "vendor/sidestep/crates/sidestep-engine/src/backend/keyboard.rs",
        "vendor/sidestep/crates/sidestep-engine/src/backend/seat.rs",
        "vendor/sidestep/crates/sidestep-ui/src/event.rs", "vendor/sidestep/crates/sidestep-ui/src/testing.rs",
        "apps/hotaru/upstream/sidestep-clipboard.patch", "apps/hotaru/upstream/sidestep-input.patch",
    ])
    binary_hash = hashlib.sha256(binary.read_bytes()).hexdigest()
    manifest = {"recorded_utc": datetime.now(timezone.utc).isoformat(), "binary": str(binary),
                "binary_sha256": binary_hash,
                "sources": {str(path.relative_to(root)): hashlib.sha256(path.read_bytes()).hexdigest()
                            for path in sorted(source_paths)}}
    (evidence / "source-manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    build = evidence / "keyboard"
    build.mkdir(exist_ok=True)
    xml = next(Path.home().glob(".cargo/registry/src/*/wayland-protocols-misc-*/protocols/virtual-keyboard-unstable-v1.xml"))
    subprocess.run(["wayland-scanner", "client-header", str(xml), str(build / "virtual-keyboard-client.h")], check=True)
    subprocess.run(["wayland-scanner", "private-code", str(xml), str(build / "virtual-keyboard.c")], check=True)
    flags = subprocess.check_output(["pkg-config", "--cflags", "--libs", "wayland-client", "xkbcommon"], text=True).split()
    subprocess.run(["cc", "-O2", "-Wall", "-Wextra", "-Werror", "-I" + str(build), str(Path(__file__).with_name("key.c")),
                    str(build / "virtual-keyboard.c"), "-o", str(build / "key"), *flags], check=True)
    pointer_xml = next(Path.home().glob(".cargo/registry/src/*/wayland-protocols-wlr-*/wlr-protocols/unstable/wlr-virtual-pointer-unstable-v1.xml"))
    subprocess.run(["wayland-scanner", "client-header", str(pointer_xml), str(build / "virtual-pointer-client.h")], check=True)
    subprocess.run(["wayland-scanner", "private-code", str(pointer_xml), str(build / "virtual-pointer.c")], check=True)
    subprocess.run(["cc", "-O2", "-Wall", "-Wextra", "-Werror", "-I" + str(build), str(Path(__file__).with_name("pointer.c")),
                    str(build / "virtual-pointer.c"), "-o", str(build / "pointer"), *flags], check=True)
    runtime = Path(tempfile.mkdtemp(prefix="hotaru-wayland-"))
    os.chmod(runtime, 0o700)
    processes = []
    files = []
    result = {"backend": "private headless Sway, pixman", "binary": str(binary),
              "binary_sha256": binary_hash, "source_manifest": "source-manifest.json", "checks": {}}
    env = dict(os.environ)
    for key in ["DISPLAY", "WAYLAND_DISPLAY", "SWAYSOCK", "AIZOME_SHELL_SOCKET", "AIZOME_SOCKET", "SIDESTEP_BACKEND"]:
        env.pop(key, None)
    env.update(XDG_RUNTIME_DIR=str(runtime), XDG_CONFIG_HOME=str(runtime / "config"), WLR_BACKENDS="headless", WLR_RENDERER="pixman",
               WLR_LIBINPUT_NO_DEVICES="1", XDG_CURRENT_DESKTOP="sway", XDG_SESSION_TYPE="wayland")
    config = runtime / "sway.conf"
    config.write_text("output HEADLESS-1 mode 1280x800\nseat seat0 fallback true\nfocus_follows_mouse no\n")
    inspect = evidence / "inspect"
    inspect.mkdir(exist_ok=True)
    env["AIZOME_KIT_INSPECT"] = str(inspect)
    try:
        swaylog = open(evidence / "sway.log", "wb"); files.append(swaylog)
        compositor = subprocess.Popen(["sway", "--unsupported-gpu", "-c", str(config)], env=env, stdout=swaylog, stderr=subprocess.STDOUT)
        processes.append(compositor)
        display = wait("private Wayland socket", lambda: next((p for p in runtime.glob("wayland-*") if p.is_socket()), None))
        socket = wait("private Sway socket", lambda: next(runtime.glob("sway-ipc.*.sock"), None))
        env.update(WAYLAND_DISPLAY=display.name, SWAYSOCK=str(socket))

        def sway(*args):
            return json.loads(subprocess.check_output(["swaymsg", "-s", str(socket), *args], env=env))

        def key(code, modifiers, layout=None):
            key_env = dict(env)
            if layout:
                key_env["HOTARU_TEST_LAYOUT"] = layout
            subprocess.run([str(build / "key"), str(code), modifiers], env=key_env, check=True, timeout=3)

        def click(x, y):
            subprocess.run([str(build / "pointer"), str(int(x)), str(int(y)), "1280", "800"],
                           env=env, check=True, timeout=3)

        def tree_nodes(node):
            yield node
            for child in node.get("nodes", []) + node.get("floating_nodes", []):
                yield from tree_nodes(child)

        expected = b"".join(f"{index:05d} ".encode() + b"A" * 121 + b"\n" for index in range(9000))
        (evidence / "expected-output.txt").write_bytes(expected)
        pasted = b"hotaru-paste-roundtrip-" * 50000
        (evidence / "expected-paste.txt").write_bytes(pasted)
        expected_input = (b"\x1b[97;5u\x1b[97;5:3u"
                          b"\x1b[49:33;2;33u\x1b[49:33;2:3u"
                          b"\x1b[122:90:121;2;90u\x1b[122:90:121;2:3u"
                          b"\x1b[57442;5u\x1b[57442;5:3u"
                          b"\x1b[<0;1;1M\x1b[<0;1;1m")
        (evidence / "expected-input.bin").write_bytes(expected_input)
        fixture = runtime / "fixture.py"
        fixture.write_text("""import os, pathlib, sys, time, tty
root = pathlib.Path(sys.argv[1])
while not (root / 'produce').exists(): time.sleep(.01)
data = (root / 'expected-output.txt').read_bytes()
while data:
    n = os.write(1, data)
    data = data[n:]
tty.setraw(0)
(root / 'ready').touch()
expected = (root / 'expected-paste.txt').stat().st_size
data = bytearray()
while len(data) < expected:
    chunk = os.read(0, min(65536, expected - len(data)))
    if not chunk: break
    data.extend(chunk)
(root / 'received-paste.txt').write_bytes(data)
os.write(1, b'\\r\\npaste-roundtrip-ok\\r\\n')
while not (root / 'request-input').exists(): time.sleep(.01)
os.write(1, b'\\x1b[>31u\\x1b[?1000h\\x1b[?1006h')
(root / 'input-ready').touch()
expected = (root / 'expected-input.bin').stat().st_size
data = bytearray()
while len(data) < expected:
    chunk = os.read(0, min(65536, expected - len(data)))
    if not chunk: break
    data.extend(chunk)
    (root / 'received-input-progress.bin').write_bytes(data)
(root / 'received-input.bin').write_bytes(data)
os.write(1, b'\\r\\nnative-input-roundtrip-ok\\r\\n')
while True: time.sleep(1)
""")
        for marker in ["produce", "ready", "received-paste.txt", "request-input", "input-ready", "received-input.bin", "received-input-progress.bin"]:
            (evidence / marker).unlink(missing_ok=True)
        applog = open(evidence / "hotaru.log", "wb"); files.append(applog)
        app = subprocess.Popen([str(binary), "--", sys.executable, str(fixture), str(evidence)], env=env, stdout=applog, stderr=subprocess.STDOUT)
        processes.append(app)
        window = wait("Hotaru Wayland window", lambda: next((n for n in tree_nodes(sway("-t", "get_tree")) if n.get("app_id") == "org.aizome.Hotaru"), None))
        sway(f"[con_id={window['id']}] focus")

        def inspected():
            try:
                path = next(inspect.glob(f"org.aizome.Hotaru.{app.pid}.*.json"))
                return json.loads(path.read_text())
            except (StopIteration, json.JSONDecodeError, FileNotFoundError):
                return None

        wait("native terminal inspection", inspected)
        time.sleep(.3)
        (evidence / "produce").touch()
        wait("PTY fixture output complete", lambda: (evidence / "ready").exists())
        time.sleep(.3)
        key(46, "super,shift")  # C: Copy All Output, native menu command.

        copied = None
        until = time.monotonic() + 10
        while time.monotonic() < until:
            outcome = subprocess.run(["wl-paste", "--no-newline"], env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=3)
            if outcome.returncode == 0 and outcome.stdout.rstrip(b"\n") == expected.rstrip(b"\n"):
                copied = outcome.stdout; break
            time.sleep(.05)
        if copied is None:
            (evidence / "failed-copy.txt").write_bytes(outcome.stdout)
            (evidence / "failed-copy-inspect.json").write_text(json.dumps(inspected(), indent=2))
            subprocess.run(["grim", str(evidence / "failed-copy.png")], env=env, timeout=3)
            raise AssertionError(f"Native copy mismatch: {len(outcome.stdout)} bytes, expected {len(expected)}; {outcome.stderr!r}")
        (evidence / "copied.txt").write_bytes(copied)
        result["checks"]["copy"] = {"bytes": len(copied), "sha256": hashlib.sha256(copied).hexdigest(), "exact_trimmed_match": True}

        # A destination that stops reading must not hold the source event loop.
        slow = subprocess.Popen(["wl-paste", "--no-newline"], env=env, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
        processes.append(slow)
        time.sleep(.15)
        started = time.monotonic()
        key(33, "super")  # F: keep servicing native UI while clipboard pipe is full.
        wait("Find responds while copy destination is stalled", lambda: '"name": "find"' in json.dumps(inspected()))
        result["checks"]["stalled_reader_ui"] = {"seconds": time.monotonic() - started}
        slow.terminate(); slow.wait(timeout=3)

        # Refocus terminal via its inspect rectangle after Find took the focus.
        description = inspected()
        elements = description.get("elements", [])
        terminal = next(e for e in elements if e.get("name", "").startswith("terminal-"))
        x, y, width, height = terminal["rect"]
        rect = window["rect"]
        click(rect['x'] + x + width / 2, rect['y'] + y + height / 2)
        owner = subprocess.Popen(["wl-copy", "--foreground", "--type", "text/plain;charset=utf-8"],
                                 env=env, stdin=subprocess.PIPE, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        processes.append(owner)
        owner.stdin.write(pasted)
        owner.stdin.close()
        time.sleep(.1)
        key(47, "super")  # V: asynchronous native read -> PTY actor.
        wait("native paste reaches PTY intact", lambda: (evidence / "received-paste.txt").exists(), timeout=10)
        received = (evidence / "received-paste.txt").read_bytes()
        assert received == pasted, (len(received), len(pasted))
        result["checks"]["paste"] = {"bytes": len(received), "sha256": hashlib.sha256(received).hexdigest(), "exact_match": True}
        subprocess.run(["grim", str(evidence / "hotaru-wayland.png")], env=env, check=True, timeout=3)
        def panes():
            return [element for element in inspected().get("elements", [])
                    if element.get("name", "").startswith("terminal-")]
        key(32, "super")  # D: split the active pane to the right.
        wait("two independent native terminal panes", lambda: len(panes()) == 2)
        key(32, "super,shift")  # Shift-D: split the newly focused pane down.
        wait("three native terminal panes", lambda: len(panes()) == 3)
        subprocess.run(["grim", str(evidence / "hotaru-splits.png")], env=env, check=True, timeout=3)
        key(17, "super")  # W closes only the focused pane.
        wait("close collapses one split", lambda: len(panes()) == 2)
        key(17, "super")
        wait("original pane survives split teardown", lambda: len(panes()) == 1)
        result["checks"]["splits"] = {"created_panes": 3, "remaining_panes": 1}
        (evidence / "request-input").touch()
        wait("terminal negotiates extended input", lambda: (evidence / "input-ready").exists())
        time.sleep(.2)
        key(30, "ctrl")
        key(2, "shift")
        key(21, "shift", "de")
        key(29, "ctrl")
        terminal = panes()[0]
        x, y, _, _ = terminal["rect"]
        click(rect['x'] + x + 12, rect['y'] + y + 12)
        wait("native keyboard and mouse reach PTY", lambda: (evidence / "received-input.bin").exists())
        received_input = (evidence / "received-input.bin").read_bytes()
        assert received_input == expected_input, (received_input, expected_input)
        result["checks"]["native_input"] = {"bytes": len(received_input),
            "sha256": hashlib.sha256(received_input).hexdigest(), "exact_match": True,
            "cases": ["Ctrl-A", "US Shift-1", "German Shift-Y", "Control modifier", "SGR mouse"]}
        result["status"] = "passed"
    except Exception as error:
        result["status"] = "failed"
        result["error"] = repr(error)
        raise
    finally:
        for process in reversed(processes):
            if process.poll() is None:
                process.terminate()
                try: process.wait(timeout=3)
                except subprocess.TimeoutExpired: process.kill(); process.wait(timeout=3)
            if process.stdout: process.stdout.close()
        for file in files: file.close()
        (evidence / "result.json").write_text(json.dumps(result, indent=2) + "\n")
        shutil.rmtree(runtime)
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    run(Path(sys.argv[1]).resolve(), Path(sys.argv[2]).resolve())
