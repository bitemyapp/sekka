#!/usr/bin/env python3
"""Exercise the actual shell's launch outcomes under a private compositor.

No host settings, service, microphone or clipboard are changed. Uses the
given compositor, shell and Koe binaries; this is component evidence.
"""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--shell", required=True)
    parser.add_argument("--compositor", default="aizome")
    parser.add_argument("--ctl", default="aizomectl")
    parser.add_argument("--koe", default="aizome-koe")
    parser.add_argument("--evidence", required=True, type=Path)
    args = parser.parse_args()
    args.evidence.mkdir(parents=True, exist_ok=True)
    executables = {name: str(Path(shutil.which(getattr(args, name)) or getattr(args, name)).resolve())
                   for name in ("shell", "compositor", "ctl", "koe")}
    children = []
    with tempfile.TemporaryDirectory(prefix="aizome-startup-") as temporary:
        root = Path(temporary)
        env = os.environ.copy()
        for name in ("DISPLAY", "WAYLAND_SOCKET", "XDG_ACTIVATION_TOKEN", "DESKTOP_STARTUP_ID"):
            env.pop(name, None)
        for key, directory in {"HOME": "home", "XDG_CONFIG_HOME": "config", "XDG_DATA_HOME": "data",
                               "XDG_STATE_HOME": "state", "XDG_CACHE_HOME": "cache", "XDG_RUNTIME_DIR": "runtime",
                               "AIZOME_KIT_INSPECT": "inspect"}.items():
            path = root / directory
            path.mkdir(mode=0o700)
            env[key] = str(path)
        env.update({"XDG_DATA_DIRS": str(root / "empty"), "XDG_CONFIG_DIRS": str(root / "empty"),
                    "AIZOME_SOCKET": str(root / "runtime/aizome.startup-test.sock"),
                    "AIZOME_SHELL_SOCKET": str(root / "runtime/aizome-shell.startup-test.sock"),
                    "WAYLAND_DISPLAY": "startup-test", "AIZOME_SESSION": "0",
                    "DBUS_SESSION_BUS_ADDRESS": "unix:path=" + str(root / "no-bus"),
                    "DBUS_SYSTEM_BUS_ADDRESS": "unix:path=" + str(root / "no-bus")})
        entries = root / "data/applications"
        entries.mkdir()
        failure = root / "delayed-failure.sh"
        failure.write_text("sleep 0.2\necho 'startup-fixture: daemon did not become ready' >&2\nexit 23\n")
        for identifier, name, command in [
            ("org.example.StartupFailure", "Startup Delayed Failure", f"sh {failure}"),
            ("org.example.StartupMissing", "Startup Missing Program", "/definitely/missing/startup-fixture"),
            ("org.aizome.Koe", "Startup Window", executables["koe"]),
        ]:
            (entries / f"{identifier}.desktop").write_text(
                f"[Desktop Entry]\nType=Application\nName={name}\nExec={command}\nStartupNotify=true\n")

        def run(command):
            return subprocess.check_output(command, env=env, text=True, stderr=subprocess.DEVNULL, timeout=10)

        def until(description, check):
            deadline = time.monotonic() + 30
            while time.monotonic() < deadline:
                try:
                    result = check()
                    if result:
                        return result
                except (subprocess.SubprocessError, json.JSONDecodeError, OSError):
                    pass
                time.sleep(0.05)
            raise AssertionError(description)

        def state():
            return json.loads(run([executables["shell"], "msg", "state"]))

        def message(value):
            run([executables["shell"], "msg", json.dumps(value)])

        def launch(name):
            message({"request": "palette", "query": name})
            until("palette query", lambda: state().get("palette") == name)
            time.sleep(0.25)  # Let the palette commit and receive the keyboard.
            run([executables["ctl"], "input", "key", "Return"])
            return until("launch recorded", lambda: next((a for a in state()["launches"] if a["name"] == name), None))

        def outcome(name, text):
            return next((a for a in state()["launches"] if a["name"] == name and text in a["outcome"]), None)

        def inspected_text():
            return "\n".join(p.read_text() for p in (root / "inspect").glob("*.json"))

        try:
            with (args.evidence / "compositor.log").open("w") as log:
                children.append(subprocess.Popen([executables["compositor"], "--backend", "headless", "--renderer", "pixman",
                    "--socket", "startup-test", "--headless-output", "1280x800@1", "--no-session", "--no-shell", "--no-xwayland"],
                    env=env, stdout=log, stderr=log))
            until("compositor socket", lambda: Path(env["AIZOME_SOCKET"]).exists())
            with (args.evidence / "shell.log").open("w") as log:
                children.append(subprocess.Popen([executables["shell"]], env=env, stdout=log, stderr=log))
            until("shell starts", state)
            time.sleep(0.5)  # The desktop-entry index is loaded asynchronously.

            launch("Startup Delayed Failure")
            failure_result = until("delayed exit reported", lambda: outcome("Startup Delayed Failure", "23"))
            assert any("could not open" in n for n in state()["notes"])
            message({"request": "open", "what": "launches"})
            until("details window", lambda: "Startup" in state()["surfaces"])
            until("actual stderr visible", lambda: "startup-fixture: daemon did not become ready" in inspected_text())
            if shutil.which("grim"):
                subprocess.run(["grim", str(args.evidence / "startup-details-live.png")], env=env, check=True)
            # Close only the test shell's details window through its ordinary close button shortcut.
            run([executables["ctl"], "input", "key", "Escape"])

            launch("Startup Missing Program")
            until("missing executable reported", lambda: outcome("Startup Missing Program", "Could not start"))
            launch("Startup Window")
            until("real Koe window observed", lambda: outcome("Startup Window", "Window opened"))
            assert not (root / "config/aizome/koe.toml").exists(), "opening the window enabled dictation"
            report = {"executables": executables, "delayed_failure": failure_result, "state": state(),
                      "coverage": "private headless compositor; actual palette, shell, launcher and integrated Koe window"}
            (args.evidence / "result.json").write_text(json.dumps(report, indent=2) + "\n")
            print("PASS: delayed failure, visible journal output, missing executable, and actual Koe window readiness")
        finally:
            # The private compositor disconnects its clients. No host process is signalled.
            for child in reversed(children):
                child.terminate()
            for child in reversed(children):
                try:
                    child.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    child.kill()
                    child.wait()


if __name__ == "__main__":
    main()
