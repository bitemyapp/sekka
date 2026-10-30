"""Exercise X11 -> satellite -> Aizome using private sockets and real requests."""

import json
import os
from pathlib import Path
import select
import subprocess
import tempfile
import time


def wait_for(description, probe):
    deadline = time.monotonic() + 15
    while time.monotonic() < deadline:
        value = probe()
        if value:
            return value
        time.sleep(0.03)
    raise AssertionError(f"Timed out: {description}")


def line(process):
    assert select.select([process.stdout], [], [], 10)[0], "X11 fixture did not reply"
    value = process.stdout.readline().strip()
    assert value, f"X11 fixture exited: {process.poll()}"
    return value


def test(scale):
    with tempfile.TemporaryDirectory(prefix="sekka-x11-fullscreen-") as tmp:
        directory = Path(tmp)
        env = os.environ.copy()
        for name in ("DISPLAY", "WAYLAND_SOCKET", "AIZOME_SOCKET", "AIZOME_RENDERER"):
            env.pop(name, None)
        env.update(XDG_RUNTIME_DIR=tmp, WAYLAND_DISPLAY="test", HOME=tmp,
                   XDG_CONFIG_HOME=tmp, XDG_STATE_HOME=tmp, XDG_DATA_HOME=tmp,
                   XDG_CACHE_HOME=tmp)
        socket = directory / "aizome.test.sock"
        logs = []
        children = []

        def start(argv, **kwargs):
            child = subprocess.Popen(argv, env=env, **kwargs)
            children.append(child)
            return child

        def ipc(*args):
            result = subprocess.run(
                [os.environ["AIZOMECTL"], "--socket", str(socket), "--json", *args],
                env=env, capture_output=True, text=True, timeout=20, check=True)
            return json.loads(result.stdout)

        def window(name):
            return next((w for w in ipc("windows")["windows"] if w["app_id"] == name), None)

        try:
            for name in ("compositor", "satellite"):
                logs.append(open(directory / f"{name}.log", "w"))
            start([os.environ["AIZOME"], "--backend", "headless", "--renderer", "pixman",
                   "--headless-output", f"{1280 * scale}x{800 * scale}@{scale}",
                   "--socket", "test", "--no-session", "--no-shell", "--no-xwayland"],
                  stdout=logs[0], stderr=subprocess.STDOUT)
            wait_for("private compositor", socket.exists)
            ipc("outputs")
            number = next(n for n in range(150, 200)
                          if not Path(f"/tmp/.X11-unix/X{n}").exists()
                          and not Path(f"/tmp/.X{n}-lock").exists())
            env["DISPLAY"] = f":{number}"
            start([os.environ["SATELLITE"], env["DISPLAY"]],
                  stdout=logs[1], stderr=subprocess.STDOUT)
            wait_for("private Xwayland", lambda: Path(f"/tmp/.X11-unix/X{number}").exists())
            # Xwayland's socket can precede satellite's WM claim. A fixture
            # opens only after the bridge has announced readiness.
            wait_for("Xwayland ready", lambda: "Connected to Xwayland" in
                     (directory / "satellite.log").read_text())

            def scenario(name, size, decoration, before, expected, transitions=False,
                         take_focus=False):
                client = start([os.environ["X11_FIXTURE"], name,
                                str(size[0] * scale), str(size[1] * scale), decoration,
                                *(["take-focus"] if take_focus else [])],
                               stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
                assert line(client) == "ready"

                def command(value):
                    client.stdin.write(value + "\n")
                    client.stdin.flush()
                    return line(client)

                def check(fullscreen):
                    w = wait_for(f"{name}: fullscreen={fullscreen}",
                                 lambda: (w if (w := window(name)) and
                                          w["fullscreen"] == fullscreen else None))
                    if fullscreen:
                        assert w["geometry"] == {"x": 0, "y": 0, "w": 1280, "h": 800}, w
                    wait_for(f"{name}: X11 state matches compositor",
                             lambda: command("state") == str(int(fullscreen)))

                for request in before:
                    command(request)
                # Guarantee the pre-map messages were processed without a
                # Wayland role, instead of relying on a lucky startup race.
                time.sleep(0.15)
                command("map")
                check(expected)
                if transitions:
                    for _ in range(3):
                        command("minimize")
                        wait_for(f"{name}: minimized", lambda: window(name)["minimised"])
                        wait_for(f"{name}: ICCCM iconic", lambda: command("iconic") == "1")
                        wait_for(f"{name}: EWMH hidden", lambda: command("hidden") == "1")
                        # SDL drops the fullscreen request while hidden. The
                        # fullscreen acknowledgement must preserve HIDDEN.
                        command("remove")
                        check(False)
                        assert command("hidden") == "1"
                        ipc("focus", str(window(name)["id"]))
                        wait_for(f"{name}: restored", lambda: not window(name)["minimised"])
                        wait_for(f"{name}: ICCCM normal", lambda: command("iconic") == "0")
                        wait_for(f"{name}: EWMH visible", lambda: command("hidden") == "0")
                        command("add")
                        check(True)
                    command("remove")
                    check(False)
                    command("add")
                    check(True)
                    command("toggle")
                    check(False)
                    command("unmap")
                    wait_for(f"{name}: unmapped", lambda: window(name) is None)
                    command("property-full")
                    command("map")
                    check(True)
                    command("unmap")
                    wait_for(f"{name}: unmapped again", lambda: window(name) is None)
                    command("property-normal")
                    command("map")
                    check(False)
                    command("add")
                    check(True)
                    command("unmap")
                    wait_for(f"{name}: withdrawn", lambda: window(name) is None)
                    # The bridge, as window manager, removes its own state on
                    # withdrawal; otherwise FULLSCREEN returns on remapping.
                    wait_for(f"{name}: EWMH state removed",
                             lambda: command("state-present") == "0")
                    command("restore-size")
                    command("map")
                    check(False)
                client.stdin.write("quit\n")
                client.stdin.flush()
                assert client.wait(timeout=10) == 0
                wait_for(f"{name}: closed", lambda: window(name) is None)
                print(f"PASS {scale}x {name}", flush=True)

            scenario("initial-fullscreen", (640, 480), "borderless", ["property-full"], True, True)
            scenario("take-focus-fullscreen", (640, 480), "borderless", ["property-full"], True,
                     True, take_focus=True)
            scenario("early-fullscreen", (640, 480), "borderless", ["add"], True)
            scenario("cancel-early-fullscreen", (1280, 800), "borderless", ["add", "remove"], False)
            scenario("initial-windowed", (1280, 800), "borderless", ["property-normal"], False)
            scenario("decorated-monitor-size", (1280, 800), "decorated", [], False)
            scenario("legacy-borderless", (1280, 800), "borderless", [], True)
            scenario("legacy-unmanaged", (1280, 800), "unmanaged", [], True)
            scenario("small-windowed", (640, 480), "borderless", [], False)

            # Use real RandR requests: Xwayland publishes the per-client
            # emulated monitor rectangle. The physical output must stay at
            # its desktop mode while the game's drawable retains its mode.
            name = "emulated-resolution"
            client = start([os.environ["X11_FIXTURE"], name, "640", "480", "decorated"],
                           stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
            assert line(client) == "ready"

            def command(value):
                client.stdin.write(value + "\n")
                client.stdin.flush()
                return line(client)

            command("mode 640 480")
            command("property-full")
            command("map")
            w = wait_for("emulated fullscreen mapped", lambda: window(name))
            ipc("focus", str(w["id"]))
            for width, height in [(640, 480), (800, 600), (640, 480)]:
                command(f"mode {width} {height}")
                wait_for("X11 drawable retains chosen mode",
                         lambda: command("geometry") == f"{width} {height}")
                assert window(name)["geometry"] == {"x": 0, "y": 0, "w": 1280, "h": 800}
                assert window(name)["fullscreen"]
                # Move while still inside the surface, including after a
                # mode change. Both axes differ from the output DPI scale.
                ipc("input", "motion", "900", "550")
                ipc("input", "motion", "960", "600")
                wait_for("pointer uses emulated resolution",
                         lambda: command("pointer") == f"{width * 3 // 4} {height * 3 // 4}")
            # A game that resizes away from its mode leaves emulation, as in
            # Xwayland: the viewport and pointer follow its actual drawable.
            command(f"resize {1280 * scale} {800 * scale}")
            wait_for("game resized its drawable",
                     lambda: command("geometry") == f"{1280 * scale} {800 * scale}")
            ipc("input", "motion", "900", "550")
            ipc("input", "motion", "960", "600")
            wait_for("pointer follows the resized drawable",
                     lambda: command("pointer") == f"{960 * scale} {600 * scale}")
            command(f"mode {1280 * scale} {800 * scale}")
            wait_for("removing emulation restores native drawable",
                     lambda: command("geometry") == f"{1280 * scale} {800 * scale}")
            command("remove")
            wait_for("leave emulated fullscreen", lambda: not window(name)["fullscreen"])
            command("restore-size")
            wait_for("windowed size restored", lambda: command("geometry") == "640 480")
            wait_for("normal DPI viewport restored",
                     lambda: window(name)["geometry"]["w"] == 640 // scale
                     and window(name)["geometry"]["h"] == 480 // scale)
            client.stdin.write("quit\n")
            client.stdin.flush()
            assert client.wait(timeout=10) == 0
            print(f"PASS {scale}x emulated resolution, mode changes, pointer and restore", flush=True)
        except Exception:
            for path in directory.glob("*.log"):
                print(f"{path.name}:\n{path.read_text()[-18000:]}", flush=True)
            raise
        finally:
            for child in reversed(children):
                if child.poll() is None:
                    child.terminate()
                    try:
                        child.wait(timeout=5)
                    except subprocess.TimeoutExpired:
                        child.kill()
                        child.wait()
            for log in logs:
                log.close()


if __name__ == "__main__":
    for scale in (1, 2):
        test(scale)
