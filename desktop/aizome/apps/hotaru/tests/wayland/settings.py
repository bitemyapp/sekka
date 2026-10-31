#!/usr/bin/env python3
"""Settings window, shortcut recording and the terminal's shortcuts, natively.

Usage: settings.py BINARY EVIDENCE_DIRECTORY
Uses a private disposable headless Sway, never the user's session. Keys go
through a real XKB keymap, so shifted punctuation (⇧❖[ types `{`) and the
in-window menu bar's shortcuts are what a desktop delivers.
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

# Linux evdev codes.
ESC, ONE, TWO, NINE, TAB, R, T, D, K, C, ENTER, LBRACKET, RBRACKET, COMMA = 1, 2, 3, 10, 15, 19, 20, 32, 37, 46, 28, 26, 27, 51


def run(binary, evidence):
    evidence.mkdir(parents=True, exist_ok=True)
    build = evidence / "keyboard"
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
    runtime = Path(tempfile.mkdtemp(prefix="hotaru-settings-wayland-"))
    os.chmod(runtime, 0o700)
    env = dict(os.environ)
    for name in ["DISPLAY", "WAYLAND_DISPLAY", "SWAYSOCK", "AIZOME_SHELL_SOCKET", "AIZOME_SOCKET", "SIDESTEP_BACKEND"]:
        env.pop(name, None)
    inspect = evidence / "inspect"
    shutil.rmtree(inspect, ignore_errors=True)
    inspect.mkdir()
    env.update(XDG_RUNTIME_DIR=str(runtime), XDG_CONFIG_HOME=str(runtime / "config"), WLR_BACKENDS="headless",
               WLR_RENDERER="pixman", WLR_LIBINPUT_NO_DEVICES="1", XDG_CURRENT_DESKTOP="sway", XDG_SESSION_TYPE="wayland",
               AIZOME_KIT_INSPECT=str(inspect))
    config_file = runtime / "config" / "aizome" / "hotaru.toml"
    (runtime / "sway.conf").write_text("output HEADLESS-1 mode 1280x800\nseat seat0 fallback true\nfocus_follows_mouse no\n")
    # Each tab runs this: a title of its own, output to clear, and no exit.
    fixture = runtime / "fixture.py"
    fixture.write_text("""import os, sys, time
os.write(1, f'\\x1b]0;tab-{os.getpid()}\\x07'.encode())
os.write(1, ''.join(f'line {n:03}\\r\\n' for n in range(200)).encode() + b'last line')
while True: time.sleep(1)
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

        def window(prefix):
            return next((n for n in nodes() if n.get("app_id") == "org.aizome.Hotaru" and (n.get("name") or "").startswith(prefix)), None)

        def terminal_title():
            node = next((n for n in nodes() if n.get("app_id") == "org.aizome.Hotaru" and (n.get("name") or "").endswith("— Hotaru")), None)
            return node and node["name"].removesuffix(" — Hotaru")

        def inspected(title):
            for path in inspect.glob(f"org.aizome.Hotaru.{app.pid}.*.json"):
                try:
                    state = json.loads(path.read_text())
                except (FileNotFoundError, json.JSONDecodeError):
                    continue
                if state.get("title", "").startswith(title) or (title == "terminal" and state.get("title", "").endswith("— Hotaru")):
                    return state
            return None

        def texts(title):
            state = inspected(title)
            return [item.get("text") for item in state["elements"] if item.get("text")] if state else []

        def keys(*events):
            args = [value for code, modifiers in events for value in [str(code), modifiers]]
            subprocess.run([str(build / "key"), *args], env=env, check=True, timeout=3)

        def click(title, predicate):
            node, state = window(title), inspected(title)
            element = next(item for item in state["elements"] if predicate(item))
            x, y, w, h = element["rect"]
            ox = node["rect"]["x"] + node["window_rect"]["x"]
            oy = node["rect"]["y"] + node["window_rect"]["y"]
            subprocess.run([str(build / "pointer"), str(int(ox + x + w / 2)), str(int(oy + y + h / 2)), "1280", "800"],
                           env=env, check=True, timeout=3)

        def copied_all():
            keys((C, "super,shift"))
            time.sleep(.3)
            return subprocess.run(["wl-paste", "--no-newline"], env=env, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                                  timeout=3).stdout.decode()

        def config():
            try:
                return config_file.read_text()
            except FileNotFoundError:
                return ""

        def shot(name):
            subprocess.run(["grim", str(evidence / f"{name}.png")], env=env, check=True, timeout=3)

        def brightness(node):
            data = subprocess.run(["grim", "-t", "ppm", "-"], env=env, stdout=subprocess.PIPE, check=True, timeout=3).stdout
            magic, size, depth, pixels = data.split(b"\n", 3)
            width = int(size.split()[0])
            x, y = node["rect"]["x"] + 150, node["rect"]["y"] + 400
            return sum(pixels[(y * width + x) * 3:(y * width + x) * 3 + 3]) / 3

        first = wait("first tab's title", lambda: (title := terminal_title()) and title.startswith("tab-") and title)
        wait("terminal inspection", lambda: inspected("terminal"))
        time.sleep(.3)

        # ❖K clears the scrollback and the rows above the cursor's.
        assert "line 000" in wait("output to copy", lambda: (text := copied_all()) and "line 000" in text and text)
        keys((K, "super"))
        assert wait("cleared history", lambda: copied_all().strip() == "last line")
        result["checks"]["clear_screen"] = {"after": "last line"}

        # Tabs by number and in turn; ⇧❖[ and ⇧❖] type braces on this layout.
        titles = [first]
        for _ in range(2):
            keys((T, "super"))
            titles.append(wait("new tab", lambda: (title := terminal_title()) and title.startswith("tab-") and title not in titles and title))
        steps = [((ONE, "super"), 0), ((RBRACKET, "super,shift"), 1), ((LBRACKET, "super,shift"), 0),
                 ((LBRACKET, "super,shift"), 2), ((TWO, "super"), 1), ((NINE, "super"), 2), ((TAB, "ctrl"), 0)]
        for event, expected in steps:
            keys(event)
            wait(f"tab {expected + 1} after {event}", lambda: terminal_title() == titles[expected])
        result["checks"]["tabs"] = {"order": [expected + 1 for _, expected in steps]}

        # ⇧❖↩ zooms a split pane and back.
        keys((D, "super"))
        wait("split", lambda: any("×" in text for text in texts("terminal")))
        keys((ENTER, "super,shift"))
        wait("zoomed", lambda: "Zoomed: 1 of 2 panes" in texts("terminal"))
        shot("zoomed")
        keys((ENTER, "super,shift"))
        wait("unzoomed", lambda: "Zoomed: 1 of 2 panes" not in texts("terminal"))
        result["checks"]["zoom"] = {"panes": 2}

        # ❖, opens Settings; the theme cards and recorder work by pointer and keys.
        keys((COMMA, "super"))
        wait("Settings window", lambda: window("Hotaru Settings") and inspected("Hotaru Settings"))
        time.sleep(.3)
        shot("settings")
        (evidence / "settings-inspect.json").write_text(json.dumps(inspected("Hotaru Settings"), indent=2))
        click("Hotaru Settings", lambda item: item.get("text") == "Appearance")
        wait("Appearance page", lambda: "Blink" in texts("Hotaru Settings"))
        terminal = next(n for n in nodes() if (n.get("name") or "").endswith("— Hotaru"))
        started = time.monotonic()
        click("Hotaru Settings", lambda item: item.get("text") == "Day")
        wait("Day saved", lambda: 'theme = "day"' in config())
        # The panes in the window beside Settings follow it.
        wait("the terminal beside Settings in Day too", lambda: brightness(terminal) > 180, timeout=1)
        result["checks"]["day_beside_settings_seconds"] = round(time.monotonic() - started, 3)
        shot("settings-appearance-day")
        click("Hotaru Settings", lambda item: item.get("text") == "Shortcuts")
        wait("Shortcuts page", lambda: "New Tab" in texts("Hotaru Settings"))
        (evidence / "shortcuts-inspect.json").write_text(json.dumps(inspected("Hotaru Settings"), indent=2))
        click("Hotaru Settings", lambda item: item.get("label") == "Change the shortcuts for New Tab")
        recorder = lambda: any(item.get("label") == "New shortcut for New Tab" for item in inspected("Hotaru Settings")["elements"])
        wait("recorder", recorder)
        shot("settings-recording")
        # ⌥❖R is Reset Terminal's: recording it for New Tab moves it.
        keys((R, "super,alt"))
        wait("⌥❖R saved for New Tab", lambda: '"super+alt+r" = "new-tab"' in config())
        assert "runs New Tab now, not Reset Terminal" in " ".join(texts("Hotaru Settings"))
        keys((ESC, "none"))
        wait("recorder closed", lambda: not recorder())
        result["checks"]["settings"] = {"theme": "day", "recorded": "super+alt+r = new-tab"}

        # ❖W in Settings closes Settings, not the pane behind it.
        keys((17, "super"))
        wait("Settings closed", lambda: window("Hotaru Settings") is None)
        keys((ENTER, "super,shift"))
        wait("both panes kept", lambda: "Zoomed: 1 of 2 panes" in texts("terminal"))
        keys((ENTER, "super,shift"))
        result["checks"]["close_settings"] = {"panes_kept": 2}

        # The recorded shortcut works in the terminal, and ⇧❖, reads the file again.
        keys((R, "super,alt"))
        titles.append(wait("tab from ⌥❖R", lambda: (title := terminal_title()) and title.startswith("tab-") and title not in titles and title))
        size = lambda: next((text for text in texts("terminal") if " × " in text), None)
        before = wait("grid size", size)
        config_file.write_text(config().replace("font_size = 11.0", "font_size = 20.0"))
        keys((COMMA, "super,shift"))
        after = wait("reloaded larger text", lambda: (now := size()) and now != before and now)
        shot("reloaded")
        result["checks"]["reload"] = {"before": before, "after": after}
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
