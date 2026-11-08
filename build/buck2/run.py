#!/usr/bin/env python3
"""Run declared Cargo outputs without changing compiler action inputs."""
import argparse
import os
from pathlib import Path
import subprocess
import sys
import tempfile


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--environment", required=True)
    parser.add_argument("--runtime", required=True)
    parser.add_argument("--run", required=True)
    parser.add_argument("--action-module", required=True)
    parser.add_argument("--test-run", action="store_true")
    argv = sys.argv[1:]
    arguments = []
    if "--" in argv:
        delimiter = argv.index("--")
        argv, arguments = argv[:delimiter], argv[delimiter + 1:]
    args = parser.parse_args(argv)
    # Buck exports these source artifacts into separate directories.
    sys.dont_write_bytecode = True
    sys.path.insert(0, str(Path(args.action_module).absolute().parent))
    from action import load, native_environment, relocate

    tools = load(args.environment)
    root = Path.cwd()
    output = Path(args.run).absolute()
    info = relocate(load(output / "unit.json"), root, output)
    runtime = load(args.runtime)
    env = native_environment(tools)
    env.update(runtime)
    env["LD_LIBRARY_PATH"] = ":".join([*info["library_paths"], tools["rustlib"], runtime.get("LD_LIBRARY_PATH", "")])
    env.update({key: value for key, value in os.environ.items() if key.startswith(("AIZOME_", "XDG_", "WAYLAND_", "DBUS_")) or key in {"HOME", "TMPDIR", "RUST_BACKTRACE"}})
    if info.get("test_tmp"):
        (root / info["test_tmp"]).mkdir(parents=True, exist_ok=True, mode=0o700)
        env["CARGO_TARGET_TMPDIR"] = str(Path("/proc/self/cwd") / info["test_tmp"])
    command = [str(output / info["filename"]), *arguments]
    if args.test_run:
        with tempfile.TemporaryDirectory(prefix="sb-", dir="/tmp") as private:
            for name in ["home", "run", "tmp", "target"]:
                Path(private, name).mkdir(mode=0o700)
            env.update(HOME=private + "/home", XDG_RUNTIME_DIR=private + "/run", TMPDIR=private + "/tmp", CARGO_TARGET_DIR=private + "/target", DBUS_SESSION_BUS_ADDRESS="unix:path=" + private + "/no-session-bus", DBUS_SYSTEM_BUS_ADDRESS="unix:path=" + private + "/no-system-bus")
            raise SystemExit(subprocess.run(command, env=env, cwd=root).returncode)
    os.execve(command[0], command, env)


if __name__ == "__main__":
    main()
