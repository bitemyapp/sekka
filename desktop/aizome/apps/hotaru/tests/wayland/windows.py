#!/usr/bin/env python3
"""Hotaru windows and Super+Return in a private headless Aizome session.

One Hotaru process runs every window of a display. Super+Return names the
window it wants a tab in by the compositor's identifier for it (Aizome's
toplevel identity protocol); this drives real keyboard shortcuts, reads
compositor state and rendered widget inspection (which carries the same
identifiers), and runs commands in actual PTYs. Never uses the live desktop.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--hotaru", required=True)
    parser.add_argument("--compositor", required=True)
    parser.add_argument("--ctl", required=True)
    parser.add_argument("--evidence", required=True, type=Path)
    args = parser.parse_args()
    binaries = {name: str(Path(getattr(args, name)).resolve()) for name in ("hotaru", "compositor", "ctl")}
    args.evidence.mkdir(parents=True, exist_ok=True)
    result = {"binaries": {name: {"path": path, "sha256": hashlib.sha256(Path(path).read_bytes()).hexdigest()}
                           for name, path in binaries.items()}, "checks": [], "status": "failed"}
    children = []
    with tempfile.TemporaryDirectory(prefix="hotaru-desks-") as temporary:
        root = Path(temporary)
        env = os.environ.copy()
        for key in ("DISPLAY", "WAYLAND_SOCKET", "XDG_ACTIVATION_TOKEN", "DESKTOP_STARTUP_ID", "SWAYSOCK", "SIDESTEP_BACKEND"):
            env.pop(key, None)
        for key, name in {"HOME": "home", "XDG_CONFIG_HOME": "config", "XDG_DATA_HOME": "data",
                          "XDG_STATE_HOME": "state", "XDG_CACHE_HOME": "cache", "XDG_RUNTIME_DIR": "runtime",
                          "AIZOME_KIT_INSPECT": "inspect"}.items():
            directory = root / name
            directory.mkdir(mode=0o700)
            env[key] = str(directory)
        env.update(WAYLAND_DISPLAY="hotaru-desks", AIZOME_SOCKET=str(root / "runtime/aizome.hotaru-desks.sock"),
                   AIZOME_SHELL_SOCKET=str(root / "runtime/no-shell.sock"), AIZOME_SESSION="0",
                   XDG_CURRENT_DESKTOP="Aizome", XDG_CONFIG_DIRS=str(root / "empty"), XDG_DATA_DIRS=str(root / "empty"),
                   DBUS_SESSION_BUS_ADDRESS="unix:path=" + str(root / "no-bus"),
                   DBUS_SYSTEM_BUS_ADDRESS="unix:path=" + str(root / "no-bus"))
        (root / "data/applications").mkdir()
        (root / "data/applications/org.aizome.Hotaru.desktop").write_text(
            "[Desktop Entry]\nType=Application\nName=Hotaru\nCategories=System;TerminalEmulator;\n"
            f"Exec={binaries['hotaru']}\nTerminal=false\n")
        (root / "config/aizome").mkdir()
        (root / "config/aizome/hotaru.toml").write_text(
            f"shell = {json.dumps(shutil.which('sh'))}\nshell_integration = false\n")
        (root / "config/aizome/settings.toml").write_text("[windows]\nstack_new_windows_of_same_app = false\n")
        (root / "config/xdg-terminals.list").write_text("org.aizome.Hotaru.desktop\n")
        for name in ("desk-one", "desk-two"):
            (root / name).mkdir()

        def ctl(*arguments):
            return subprocess.check_output([binaries["ctl"], *arguments], env=env, text=True, timeout=10)

        def wait(description, check):
            deadline = time.monotonic() + 25
            while time.monotonic() < deadline:
                try:
                    value = check()
                    if value:
                        return value
                except (OSError, subprocess.SubprocessError, json.JSONDecodeError, StopIteration, KeyError):
                    pass
                time.sleep(0.05)
            raise AssertionError(description)

        def windows():
            return [w for w in json.loads(ctl("--json", "windows"))["windows"] if w["app_id"] == "org.aizome.Hotaru" and w["parent"] is None]

        def front():
            return next((w for w in windows() if w["focused"]), None)

        def inspected(window):
            # The compositor's identifier for the window is the one Hotaru
            # heard: it names the window's inspection too.
            for path in (root / "inspect").glob(f"org.aizome.Hotaru.{window['pid']}.*.json"):
                doc = json.loads(path.read_text())
                if doc.get("identifier") == window["identifier"]:
                    return doc
            raise KeyError(window["identifier"])

        def tabs(window):
            return len([e for e in inspected(window)["elements"] if e.get("name", "").startswith("tab-")])

        def chord(keys):
            ctl("input", "chord", keys)

        def desk(number):
            ctl("desk", str(number))
            wait("desk switch", lambda: any(d["index"] == number and d["current"] for d in json.loads(ctl("--json", "desks"))["desks"]))

        def command(text, output, expected):
            ctl("input", "type", text)
            ctl("input", "key", "Return")
            wait("PTY command: " + text, lambda: output.exists() and output.read_text().strip() == expected)

        try:
            with (args.evidence / "compositor.log").open("w") as log:
                compositor = subprocess.Popen([binaries["compositor"], "--backend", "headless", "--renderer", "pixman",
                    "--socket", "hotaru-desks", "--headless-output", "1280x800@1", "--no-session", "--no-shell", "--no-xwayland"],
                    env=env, stdout=log, stderr=log)
                children.append(compositor)
            wait("private compositor", lambda: Path(env["AIZOME_SOCKET"]).exists())
            ctl("set-desk-folder", "1", str(root / "desk-one"))
            ctl("set-desk-folder", "2", str(root / "desk-two"))
            chord("super+Return")
            first = wait("first window on desk one", lambda: front())
            assert first["desk"] == 1 and first["identifier"]
            wait("first pane", lambda: tabs(first) == 1)
            marker = root / "one-cwd"
            command(f"pwd > {marker}", marker, str(root / "desk-one"))
            chord("super+Return")
            wait("second tab in the same window", lambda: tabs(first) == 2)
            assert len(windows()) == 1 and front()["id"] == first["id"]
            result["checks"].append("Super+Return reuses a window on the current desk")

            desk(2)
            chord("super+Return")
            second = wait("new window on desk two", lambda: len(windows()) == 2 and front())
            assert second["desk"] == 2 and second["identifier"] != first["identifier"]
            assert second["pid"] == first["pid"], "one process runs both windows"
            wait("second window pane", lambda: tabs(second) == 1)
            marker = root / "two-cwd"
            command(f"pwd > {marker}", marker, str(root / "desk-two"))
            chord("super+Return")
            wait("desk two gains a tab", lambda: tabs(second) == 2)
            assert tabs(first) == 2 and front()["id"] == second["id"]
            result["checks"].append("a desk without Hotaru gets a window of the same process, in the desk's folder")

            desk(1)
            chord("super+Return")
            wait("returning to desk one selects its window", lambda: tabs(first) == 3)
            assert tabs(second) == 2 and len(windows()) == 2
            chord("super+n")
            third = wait("New Window shortcut", lambda: len(windows()) == 3 and front())
            assert third["desk"] == 1 and third["id"] not in (first["id"], second["id"])
            assert third["pid"] == first["pid"]
            wait("new window's pane", lambda: tabs(third) == 1)
            chord("super+Return")
            wait("most recently used window gets the tab", lambda: tabs(third) == 2)
            assert tabs(first) == 3
            ctl("focus", str(first["id"]))
            chord("super+Return")
            wait("focus changes the target", lambda: tabs(first) == 4)
            result["checks"].append("New Window and most-recent window selection")

            ctl("minimise", str(first["id"]))
            ctl("minimise", str(third["id"]))
            chord("super+Return")
            restored = wait("minimised window restored by the launch's activation token", front)
            assert restored["id"] in (first["id"], third["id"]) and not restored["minimised"]
            assert len(windows()) == 3
            result["checks"].append("minimised windows are restored instead of duplicated")

            ctl("hide-app", "org.aizome.Hotaru")
            chord("super+Return")
            wait("hidden window restored", front)
            assert len(windows()) == 3
            chord("super+comma")
            wait("Settings is a transient window", lambda: any(
                w["title"] == "Hotaru Settings" and w["parent"] is not None
                for w in json.loads(ctl("--json", "windows"))["windows"]))
            chord("super+w")
            result["checks"].append("hidden windows restore and Settings remains attached to its terminal")

            # Moving the only window away changes which desk needs a new one.
            desk(2)
            ctl("move-to-desk", "3", str(second["id"]))
            chord("super+Return")
            replacement = wait("replacement on desk two", lambda: len(windows()) == 4 and front())
            assert replacement["desk"] == 2 and replacement["id"] != second["id"]
            desk(3)
            chord("super+Return")
            wait("moved window reused on desk three", lambda: tabs(second) == 3)
            assert len(windows()) == 4
            result["checks"].append("routing follows moved windows")

            # Closing a whole window cannot touch the other windows' PTYs.
            ctl("close", str(second["id"]))
            wait("one window closes", lambda: len(windows()) == 3)
            desk(2)
            chord("super+Return")
            wait("surviving window still accepts tabs", lambda: tabs(replacement) == 2)
            marker = root / "survived"
            command(f"printf survived > {marker}", marker, "survived")
            result["checks"].append("closing one window preserves other windows and their PTYs")

            ctl("close", str(first["id"]))
            wait("the window that created New Window closes", lambda: len(windows()) == 2)
            ctl("focus", str(third["id"]))
            marker = root / "child-survived"
            command(f"printf independent > {marker}", marker, "independent")
            result["checks"].append("a New Window remains usable after its creating window closes")
            desk(2)
            ctl("focus", str(replacement["id"]))

            # A command line naming a window by its identifier, as
            # Super+Return does, opens a tab there with its own directory.
            forwarded = root / "forwarded-cwd"
            subprocess.run([binaries["hotaru"], "--window", replacement["identifier"], "--working-directory",
                str(root / "desk-one"), "--", shutil.which("sh"), "-c", f"pwd > {forwarded}; sleep 60"],
                env=env, check=True, timeout=5)
            wait("explicit command reaches the named window", lambda: tabs(replacement) == 3)
            wait("explicit working directory survives handoff", lambda: forwarded.exists() and forwarded.read_text().strip() == str(root / "desk-one"))
            chord("super+w")
            wait("Close Pane closes just the command's tab", lambda: tabs(replacement) == 2)
            # A name no window has (a window closed since) gets a window.
            subprocess.run([binaries["hotaru"], "--window", first["identifier"]], env=env, check=True, timeout=5)
            stray = wait("a closed window's name opens a window", lambda: len(windows()) == 3 and front())
            assert stray["pid"] == first["pid"] and stray["id"] not in (replacement["id"], third["id"])
            ctl("close", str(stray["id"]))
            wait("stray window closes", lambda: len(windows()) == 2)
            result["checks"].append("named handoff preserves arguments and directory; a stale name opens a window")

            # A second compositor shares the runtime directory: its Hotaru is
            # its own, and Quit stays in its own Wayland session.
            other_env = {**env, "WAYLAND_DISPLAY": "hotaru-other", "AIZOME_SOCKET": str(root / "runtime/aizome.hotaru-other.sock")}
            with (args.evidence / "other-session.log").open("w") as log:
                other_compositor = subprocess.Popen([binaries["compositor"], "--backend", "headless", "--renderer", "pixman",
                    "--socket", "hotaru-other", "--headless-output", "800x600@1", "--no-session", "--no-shell", "--no-xwayland"],
                    env=other_env, stdout=log, stderr=log)
                children.append(other_compositor)
                wait("other private display", lambda: Path(other_env["AIZOME_SOCKET"]).exists())
                other_hotaru = subprocess.Popen([binaries["hotaru"], "--new-window"], env=other_env, stdout=log, stderr=log)
                children.append(other_hotaru)

            def other_windows():
                return json.loads(subprocess.check_output([binaries["ctl"], "--json", "windows"], env=other_env, text=True, timeout=5))["windows"]

            wait("other session's Hotaru", other_windows)
            assert other_windows()[0]["pid"] != first["pid"], "each display has a Hotaru of its own"
            subprocess.run([binaries["hotaru"]], env=other_env, check=True, timeout=5)
            wait("an ordinary launch opens another window", lambda: len(other_windows()) == 2)
            assert len({w["pid"] for w in other_windows()}) == 1
            result["checks"].append("ordinary launches open windows in the display's Hotaru")

            chord("super+q")
            wait("Quit Hotaru closes all windows in this session", lambda: not windows())
            assert other_hotaru.poll() is None and len(other_windows()) == 2
            result["checks"].append("Quit closes every Hotaru window in the session")
            result["checks"].append("Quit leaves a different Wayland session untouched")

            # A different default terminal must receive its own Exec intact.
            record = root / "other-terminal-args"
            fixture = root / "other-terminal.sh"
            fixture.write_text(f"printf '%s\\n' \"$@\" > {record}\n")
            (root / "data/applications/org.example.Terminal.desktop").write_text(
                "[Desktop Entry]\nType=Application\nName=Other Terminal\nCategories=TerminalEmulator;\n"
                f"Exec={shutil.which('sh')} {fixture} original-argument\nTerminal=false\n")
            (root / "config/xdg-terminals.list").write_text("org.example.Terminal.desktop\n")
            chord("super+Return")
            wait("another default terminal keeps its arguments", lambda: record.exists() and record.read_text() == "original-argument\n")
            assert not windows()
            result["checks"].append("another default terminal keeps its launch command")
            result["status"] = "passed"
        finally:
            # Disconnect only this private compositor's clients, including
            # windows it spawned. Never signal by executable name or host PID.
            for process in reversed(children):
                process.terminate()
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait(timeout=5)
            (args.evidence / "result.json").write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
