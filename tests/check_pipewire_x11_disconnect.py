"""Exercise the real X11 bell module against a disposable X server.

No audio devices, user bus, or desktop PipeWire socket are exposed to the
isolated daemon. Logs are retained in the check output.
"""
import argparse
import json
import os
from pathlib import Path
import resource
import select
import signal
import subprocess
import tempfile
import time


def wait_for_log(process, path, text):
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline:
        if text in path.read_text():
            return
        if process.poll() is not None:
            raise AssertionError(f"daemon exited {process.returncode}: {path.read_text()}")
        time.sleep(0.05)
    raise AssertionError(f"did not observe {text!r}: {path.read_text()}")


def stop(process):
    if process is not None and process.poll() is None:
        process.terminate()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()


def no_core():
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))


def case(args, mode, iteration):
    evidence = args.output / f"{mode}-{iteration}"
    evidence.mkdir()
    with tempfile.TemporaryDirectory(prefix="pw-bell-") as temporary, \
            (evidence / "xvfb.log").open("w") as xlog, \
            (evidence / "pipewire.log").open("w") as plog:
        runtime = Path(temporary)
        config = runtime / "isolated-bell.conf"
        config.write_text("""context.properties = { core.daemon = false support.dbus = false }
context.spa-libs = { support.* = support/libspa-support }
context.modules = [ { name = libpipewire-module-x11-bell } ]
""")
        read_fd, write_fd = os.pipe()
        xserver = daemon = None
        try:
            xserver = subprocess.Popen(
                [args.xvfb, "-displayfd", str(write_fd), "-screen", "0", "640x480x24",
                 "-nolisten", "tcp"], pass_fds=[write_fd], stdout=xlog, stderr=xlog)
            os.close(write_fd)
            write_fd = None
            if not select.select([read_fd], [], [], 10)[0]:
                raise AssertionError("private X server did not initialize")
            display = ":" + os.read(read_fd, 100).decode().strip()
            os.close(read_fd)
            read_fd = None
            env = os.environ.copy()
            env.update({"DISPLAY": display, "XAUTHORITY": str(runtime / "no-authority"),
                        "XDG_RUNTIME_DIR": str(runtime), "PIPEWIRE_RUNTIME_DIR": str(runtime),
                        "PIPEWIRE_CONFIG_DIR": str(runtime), "PIPEWIRE_DEBUG": "3",
                        "DBUS_SESSION_BUS_ADDRESS": "unix:path=" + str(runtime / "no-bus")})
            env.pop("WAYLAND_DISPLAY", None)
            daemon = subprocess.Popen([args.pipewire, "-c", config.name], env=env,
                                      stdout=plog, stderr=plog, preexec_fn=no_core)
            log = evidence / "pipewire.log"
            wait_for_log(daemon, log, "loaded module libpipewire-module-x11-bell")
            wait_for_log(daemon, log, "start main loop")
            if mode != "daemon-first":
                xserver.send_signal(signal.SIGTERM if mode == "server-stop" else signal.SIGKILL)
                xserver.wait(timeout=5)
                wait_for_log(daemon, log, "has encountered a fatal I/O error")
                # Allow the main loop to run the deferred module destruction.
                try:
                    code = daemon.wait(timeout=0.5)
                except subprocess.TimeoutExpired:
                    code = None
                assert code is None, f"PipeWire died after X server exit: {code}"
            daemon.terminate()
            assert daemon.wait(timeout=5) == 0, "PipeWire did not shut down cleanly"
            result = {"mode": mode, "iteration": iteration, "pipewire": args.pipewire,
                      "xvfb": args.xvfb, "result": "pass"}
            (evidence / "result.json").write_text(json.dumps(result, indent=2) + "\n")
        finally:
            stop(daemon)
            stop(xserver)
            for fd in [read_fd, write_fd]:
                if fd is not None:
                    os.close(fd)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--pipewire", required=True)
    parser.add_argument("--xvfb", required=True)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    args.output.mkdir(parents=True)
    for mode in ["server-stop", "server-kill", "daemon-first"]:
        for iteration in range(3):
            case(args, mode, iteration)
