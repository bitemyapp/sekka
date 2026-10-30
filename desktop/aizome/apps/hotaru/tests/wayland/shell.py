#!/usr/bin/env python3
"""Packaged default-shell startup, OSC7 and split cwd inheritance.

Usage: shell.py BINARY EVIDENCE_DIRECTORY
Uses an isolated HOME and private headless Sway; runtime resources must come
from the package wrapper. Never reads or writes the user's shell configuration.
"""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

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
    runtime = Path(tempfile.mkdtemp(prefix="hotaru-shell-wayland-"))
    os.chmod(runtime, 0o700)
    home = runtime / "home"
    home.mkdir()
    target = home / "space 日本"
    target.mkdir()
    record = evidence / "startup.bin"
    record.unlink(missing_ok=True)
    # Record the actual inherited cwd before any user command; do not cd here.
    (home / ".bashrc").write_text("""PS1='HOTARU> '
PROMPT_COMMAND=
builtin printf '%s\\0%s\\0%s\\0%s\\0' "$PWD" "$TERM" "$HOTARU_RESOURCES_DIR" "$HOTARU_TERMINFO_DIR" >> "$HOTARU_TEST_STARTUP_RECORD"
""")
    env = dict(os.environ)
    for name in ["DISPLAY", "WAYLAND_DISPLAY", "SWAYSOCK", "AIZOME_SHELL_SOCKET", "AIZOME_SOCKET", "SIDESTEP_BACKEND",
                 "AIZOME_FONT_DIR", "HOTARU_FONT_DIR", "HOTARU_RESOURCES_DIR", "HOTARU_TERMINFO_DIR", "LD_LIBRARY_PATH",
                 "FONTCONFIG_FILE", "FONTCONFIG_PATH", "SIDESTEP_FONT", "SIDESTEP_MONO_FONT", "ENV", "BASH_ENV",
                 "PROMPT_COMMAND", "SHELLOPTS", "BASHOPTS"]:
        env.pop(name, None)
    inspect = evidence / "inspect"
    inspect.mkdir(exist_ok=True)
    env.update(HOME=str(home), SHELL="/usr/bin/bash", XDG_RUNTIME_DIR=str(runtime), XDG_CONFIG_HOME=str(runtime / "config"),
               XDG_CACHE_HOME=str(runtime / "cache"), XDG_DATA_HOME=str(runtime / "data"), WLR_BACKENDS="headless",
               WLR_RENDERER="pixman", WLR_LIBINPUT_NO_DEVICES="1", XDG_CURRENT_DESKTOP="sway", XDG_SESSION_TYPE="wayland",
               AIZOME_KIT_INSPECT=str(inspect), HOTARU_TEST_STARTUP_RECORD=str(record))
    # This suite checks bash's integration. Named, so a host with fish, which
    # Hotaru otherwise starts first, doesn't change what it checks.
    (runtime / "config" / "aizome").mkdir(parents=True, exist_ok=True)
    (runtime / "config" / "aizome" / "hotaru.toml").write_text('shell = "/usr/bin/bash"\n')
    config = runtime / "sway.conf"
    config.write_text("output HEADLESS-1 mode 1280x800\nseat seat0 fallback true\n")
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
        # No explicit command: exercise main's default-shell launch path.
        app = subprocess.Popen([str(binary)], cwd=home, env=env, stdout=log, stderr=subprocess.STDOUT)
        processes.append(app)

        def inspected():
            try: return json.loads(next(inspect.glob(f"org.aizome.Hotaru.{app.pid}.*.json")).read_text())
            except (StopIteration, FileNotFoundError, json.JSONDecodeError): return None

        def startups():
            try: parts = record.read_bytes().split(b"\0")
            except FileNotFoundError: return []
            if parts[-1] or (len(parts) - 1) % 4: return []
            return [[field.decode() for field in parts[n:n+4]] for n in range(0, len(parts)-1, 4)]

        def keys(events):
            args = [value for code, modifiers in events for value in [str(code), modifiers]]
            subprocess.run([str(build / "key"), *args], env=env, check=True, timeout=3)

        wait("initial shell startup", lambda: len(startups()) == 1)
        wait("initial cwd-derived native title", lambda: (inspected() or {}).get("title") == f"{home} — Hotaru")
        # Actual Wayland keyboard events type: cd "$HOME"/*<Enter>. The only
        # non-hidden entry in this disposable HOME is the space/Japanese target.
        keys([(46, "none"), (32, "none"), (57, "none"), (40, "shift"), (5, "shift"),
              (35, "shift"), (24, "shift"), (50, "shift"), (18, "shift"), (40, "shift"),
              (53, "none"), (9, "shift"), (28, "none")])
        wait("OSC7-derived native title after cd", lambda: (inspected() or {}).get("title") == f"{target} — Hotaru")
        keys([(32, "super")])
        wait("second shell startup", lambda: len(startups()) == 2)
        wait("two native panes", lambda: sum(item.get("name", "").startswith("terminal-") for item in (inspected() or {}).get("elements", [])) == 2)
        records = startups()
        assert records[0][0] == str(home), records
        assert records[1][0] == str(target), records
        assert records[0][1] == records[1][1] == "xterm-hotaru", records
        resources, terminfo = Path(records[0][2]), Path(records[0][3])
        assert resources.is_absolute() and terminfo.is_absolute()
        assert records[0][2:] == records[1][2:]
        resource_hashes = {str(path.relative_to(resources)): hashlib.sha256(path.read_bytes()).hexdigest()
                           for path in resources.rglob("*") if path.is_file()}
        assert "LICENSE-MIT" in resource_hashes and "shell-integration/zsh/.zshenv" in resource_hashes
        infocmp = subprocess.check_output(["infocmp", "-1", "-x", "-A", str(terminfo), "xterm-hotaru"], env=env, text=True)
        (evidence / "installed-terminfo.txt").write_text(infocmp)
        rebuilt = runtime / "rebuilt-terminfo"
        subprocess.run(["tic", "-x", "-o", str(rebuilt), str(resources / "hotaru.terminfo")], env=env, check=True)
        rebuilt_text = subprocess.check_output(["infocmp", "-1", "-x", "-A", str(rebuilt), "xterm-hotaru"], env=env, text=True)
        clean = lambda text: [line for line in text.splitlines() if not line.startswith("#")]
        assert clean(infocmp) == clean(rebuilt_text)
        result["checks"]["cwd_inheritance"] = {"initial": records[0][0], "after_keyboard_cd": str(target),
                                                   "split_startup": records[1][0], "panes": 2}
        result["checks"]["packaged_identity"] = {"term": records[0][1], "resources": str(resources), "terminfo": str(terminfo),
                                                    "resource_sha256": resource_hashes, "recompiled_capabilities_equal": True,
                                                    "tic_version": subprocess.check_output(["tic", "-V"], text=True).strip()}
        subprocess.run(["grim", str(evidence / "split-cwd.png")], env=env, check=True, timeout=3)
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
