#!/usr/bin/env python3
"""Programs' clipboard requests, the primary selection and panes ending, natively.

Usage: selections.py BINARY EVIDENCE_DIRECTORY
Uses a private disposable headless Sway, never the user's session. Every pane
runs a raw-mode fixture: it takes commands through a FIFO named for its pid,
writes the replies it reads and what is typed or pasted into files beside it,
and ends at Control-D, as a shell does at its prompt.
"""
import base64
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

# Linux evdev codes.
D, T, ENTER = 32, 20, 28


def run(binary, evidence):
    evidence.mkdir(parents=True, exist_ok=True)
    build = evidence / "input"
    build.mkdir(exist_ok=True)
    flags = subprocess.check_output(["pkg-config", "--cflags", "--libs", "wayland-client", "xkbcommon"], text=True).split()
    for name, source, header, xml in [
        ("key", "key.c", "virtual-keyboard",
         ".cargo/registry/src/*/wayland-protocols-misc-*/protocols/virtual-keyboard-unstable-v1.xml"),
        ("pointer", "pointer.c", "virtual-pointer",
         ".cargo/registry/src/*/wayland-protocols-wlr-*/wlr-protocols/unstable/wlr-virtual-pointer-unstable-v1.xml"),
    ]:
        protocol = next(Path.home().glob(xml))
        subprocess.run(["wayland-scanner", "client-header", str(protocol), str(build / f"{header}-client.h")], check=True)
        subprocess.run(["wayland-scanner", "private-code", str(protocol), str(build / f"{name}-protocol.c")], check=True)
        subprocess.run(["cc", "-O2", "-Wall", "-Wextra", "-Werror", "-I" + str(build), str(Path(__file__).with_name(source)),
                        str(build / f"{name}-protocol.c"), "-o", str(build / name), *flags], check=True)
    runtime = Path(tempfile.mkdtemp(prefix="hotaru-selections-wayland-"))
    os.chmod(runtime, 0o700)
    panes = runtime / "panes"
    panes.mkdir()
    env = dict(os.environ)
    for name in ["DISPLAY", "WAYLAND_DISPLAY", "SWAYSOCK", "AIZOME_SHELL_SOCKET", "AIZOME_SOCKET", "SIDESTEP_BACKEND"]:
        env.pop(name, None)
    inspect = evidence / "inspect"
    shutil.rmtree(inspect, ignore_errors=True)
    inspect.mkdir()
    env.update(XDG_RUNTIME_DIR=str(runtime), XDG_CONFIG_HOME=str(runtime / "config"), WLR_BACKENDS="headless",
               WLR_RENDERER="pixman", WLR_LIBINPUT_NO_DEVICES="1", XDG_CURRENT_DESKTOP="sway", XDG_SESSION_TYPE="wayland",
               AIZOME_KIT_INSPECT=str(inspect), HOTARU_FIXTURE_PANES=str(panes))
    (runtime / "sway.conf").write_text("output HEADLESS-1 mode 1280x800\nseat seat0 fallback true\nfocus_follows_mouse no\n")
    fixture = runtime / "fixture.py"
    fixture.write_text(r"""import base64, os, pathlib, threading, tty
here = pathlib.Path(os.environ['HOTARU_FIXTURE_PANES']) / str(os.getpid())
here.mkdir()
tty.setraw(0)
os.write(1, f'\x1b]0;pane-{os.getpid()}\x07\x1b[2J\x1b[Halpha selected omega\r\n'.encode())
fifo = here / 'commands'
os.mkfifo(fifo)
def commands():
    while True:
        for line in open(fifo):
            verb, tag, *data = line.split()
            if verb == 'write':
                os.write(1, b'\x1b]52;' + tag.encode() + b';' + base64.b64encode(bytes.fromhex(data[0])) + b'\x07')
            elif verb == 'read':
                os.write(1, b'\x1b]52;' + tag.encode() + b';?\x07')
threading.Thread(target=commands, daemon=True).start()
pending, replies = b'', 0
while True:
    chunk = os.read(0, 65536)
    if not chunk:
        break
    pending += chunk
    while pending:
        if pending.startswith(b'\x1b]52;'):
            end = pending.find(b'\x1b\\')
            if end < 0:
                break
            replies += 1
            (here / f'reply-{replies}').write_bytes(pending[:end + 2])
            pending = pending[end + 2:]
        elif pending[:1] == b'\x04':
            raise SystemExit(0)
        else:
            with open(here / 'typed', 'ab') as typed:
                typed.write(pending[:1])
            pending = pending[1:]
""")
    processes, files = [], []
    result = {"binary": str(binary), "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
              "backend": "private headless Sway, pixman", "checks": {}}
    try:
        log = open(evidence / "sway.log", "wb"); files.append(log)
        processes.append(subprocess.Popen(["sway", "--unsupported-gpu", "-c", str(runtime / "sway.conf")], env=env,
                                          stdout=log, stderr=subprocess.STDOUT))
        display = wait("private Wayland socket", lambda: next((p for p in runtime.glob("wayland-*") if p.is_socket()), None))
        socket = wait("private Sway socket", lambda: next(runtime.glob("sway-ipc.*.sock"), None))
        env.update(WAYLAND_DISPLAY=display.name, SWAYSOCK=str(socket))
        log = open(evidence / "hotaru.log", "wb"); files.append(log)
        app = subprocess.Popen([str(binary), "--", sys.executable, str(fixture)], env=env, stdout=log, stderr=subprocess.STDOUT)
        processes.append(app)

        def nodes(node=None):
            node = node or json.loads(subprocess.check_output(["swaymsg", "-t", "get_tree"], env=env))
            yield node
            for child in node.get("nodes", []) + node.get("floating_nodes", []):
                yield from nodes(child)

        def window():
            return next((n for n in nodes() if n.get("app_id") == "org.aizome.Hotaru"), None)

        def front():
            node = window()
            name = node and node.get("name") or ""
            return name.startswith("pane-") and name.removesuffix(" — Hotaru").removeprefix("pane-")

        def inspected():
            for path in inspect.glob(f"org.aizome.Hotaru.{app.pid}.*.json"):
                try:
                    state = json.loads(path.read_text())
                except (FileNotFoundError, json.JSONDecodeError):
                    continue
                if state.get("title", "").endswith("— Hotaru"):
                    return state
            return None

        def texts():
            state = inspected()
            return [item.get("text") for item in state["elements"] if item.get("text")] if state else []

        def element(predicate):
            state = inspected()
            return state and next((item for item in state["elements"] if predicate(item)), None)

        def keys(*events):
            args = [value for code, modifiers in events for value in [str(code), modifiers]]
            subprocess.run([str(build / "key"), *args], env=env, check=True, timeout=3)

        def point(item, dx=None, dy=None, button="left", clicks=1):
            node = window()
            x, y, w, h = item["rect"]
            ox = node["rect"]["x"] + node["window_rect"]["x"]
            oy = node["rect"]["y"] + node["window_rect"]["y"]
            px = x + (w / 2 if dx is None else dx)
            py = y + (h / 2 if dy is None else dy)
            subprocess.run([str(build / "pointer"), str(int(ox + px)), str(int(oy + py)), "1280", "800", button, str(clicks)],
                           env=env, check=True, timeout=3)

        def terminal():
            return element(lambda item: str(item.get("name", "")).startswith("terminal-"))

        def command(pid, line):
            with open(panes / pid / "commands", "w") as fifo:
                fifo.write(line + "\n")

        def paste(*args):
            return subprocess.run(["wl-paste", "--no-newline", *args], env=env, stdout=subprocess.PIPE,
                                  stderr=subprocess.DEVNULL, timeout=3).stdout

        def copy(text, *args):
            subprocess.run(["wl-copy", *args, text], env=env, check=True, timeout=3)

        def reply(pid, n):
            path = panes / pid / f"reply-{n}"
            return path.exists() and path.read_bytes()

        first = wait("first pane", front)
        wait("terminal inspection", terminal)
        time.sleep(.3)
        # Wayland takes a selection only with a serial from input, as
        # anyone's typing gives it.
        point(terminal(), dx=400, dy=300)

        # Writes are allowed: exact bytes, even NUL and invalid UTF-8, on the
        # clipboard; `p` on the primary selection.
        command(first, "write c 610062ff")
        assert wait("clipboard written", lambda: paste() == b"a\0b\xff")
        command(first, "write p 776f726c64")
        assert wait("primary written", lambda: paste("--primary") == b"world")
        result["checks"]["writes"] = {"clipboard": "610062ff", "primary": "world"}

        # Reads are asked about, showing what they'd get; allowed, the answer
        # is the clipboard's bytes under the program's tag.
        copy("from elsewhere")
        time.sleep(.3)
        command(first, "read c")
        wait("read question", lambda: any("read the clipboard?" in text for text in texts()))
        assert any("“from elsewhere”" in text for text in texts()), texts()
        subprocess.run(["grim", str(evidence / "read-question.png")], env=env, check=True, timeout=3)
        point(element(lambda item: item.get("text") == "Allow"))
        expected = b"\x1b]52;c;" + base64.b64encode(b"from elsewhere") + b"\x1b\\"
        assert wait("allowed reply", lambda: reply(first, 1)) == expected
        command(first, "read c")
        wait("second question", lambda: any("read the clipboard?" in text for text in texts()))
        point(element(lambda item: item.get("text") == "Don’t Allow"))
        assert wait("refused reply", lambda: reply(first, 2)) == b"\x1b]52;c;\x1b\\"
        result["checks"]["reads"] = {"allowed": expected.decode(), "refused": "empty answer"}

        # Copy on select: a double click puts the word on the primary selection;
        # a middle click pastes the primary selection into the program.
        grid = terminal()
        point(grid, dx=14, dy=16, clicks=2)
        assert wait("word selected", lambda: paste("--primary") == b"alpha"), paste("--primary")
        copy("pasted", "--primary")
        time.sleep(.3)
        point(grid, dx=14, dy=16, button="middle")
        typed = lambda: (panes / first / "typed").exists() and (panes / first / "typed").read_bytes()
        assert wait("middle-click paste", lambda: typed() and b"pasted" in typed()), typed()
        result["checks"]["primary"] = {"double_click": "alpha", "middle_click": "pasted"}

        # Control-D ends a pane's program: its pane closes, then its tab, and
        # the window with the last.
        keys((D, "super"))
        second = wait("split pane", lambda: (pid := front()) and pid != first and pid)
        keys((D, "ctrl"))
        assert wait("split closed", lambda: front() == first)
        keys((T, "super"))
        third = wait("new tab", lambda: (pid := front()) and pid not in (first, second) and pid)
        keys((D, "ctrl"))
        assert wait("tab closed", lambda: front() == first)
        assert app.poll() is None
        keys((D, "ctrl"))
        assert wait("Hotaru exited", lambda: app.poll() is not None) is not None
        result["checks"]["control_d"] = {"split": second, "tab": third, "exit_status": app.returncode}
        assert app.returncode == 0
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
