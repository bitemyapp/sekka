"""Drag and drop through xwayland-satellite, driven with the real pointer.

dragon-drop (GTK 3) is the source, as an X11 client or a Wayland one by
GDK_BACKEND, and the X11 target; Hako, which copies what is dropped on a
folder, is the Wayland target. Satellite puts every X11 window at the X
screen's origin, so a source and a target placed apart in Aizome are at
different places than on the X screen: an X11 drag between them only lands
if satellite carries it.
This component fixture does not qualify an ISO or physical hardware.
"""
import json
import shlex
import time
from pathlib import Path

FILE = "/home/alice/dropped.txt"
FOLDER = "/home/alice/drop-folder"
inputs = []


def user(command):
    return machine.succeed("su - alice -c " + shlex.quote(
        "export XDG_RUNTIME_DIR=/run/user/1000; "
        "export WAYLAND_DISPLAY=$(ls /run/user/1000 | grep -m1 '^wayland-[0-9]*$'); "
        "export AIZOME_SOCKET=/run/user/1000/aizome.$WAYLAND_DISPLAY.sock; "
        + command))


def until(description, check, timeout=20):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        value = check()
        if value:
            return value
        time.sleep(0.2)
    raise AssertionError(description)


def windows():
    return json.loads(user("aizomectl --json windows"))["windows"]


def input_(command):
    inputs.append(command)
    user("aizomectl input " + command)


def spawn(name, command, x, y, width=None, height=None):
    """A program's window, moved to x, y (and sized). Each has a name of its
    own, so that Aizome doesn't stack them as one app's windows."""
    before = {w["id"] for w in windows()}
    user("aizomectl spawn -- sh -c " + shlex.quote(
        f"{command} > /tmp/{name}.out 2> /tmp/{name}.err"))
    window = until(name + " window", lambda: next(
        (w for w in windows() if w["id"] not in before), None))
    if width:
        user(f"aizomectl resize {window['id']} {x} {y} {width} {height}")
    else:
        user(f"aizomectl move {window['id']} {x} {y}")
    return until(name + " placed", lambda: next(
        (w for w in windows() if w["id"] == window["id"]
         and (w["geometry"]["x"], w["geometry"]["y"]) == (x, y)), None))


def dragon(name, backend, arguments, x, y):
    return spawn(name, f"GDK_BACKEND={backend} exec -a {name} dragon-drop {arguments}", x, y)


def drag(source, target):
    s, t = source["geometry"], target["geometry"]
    # The file's button fills the source's lower part.
    sx, sy = s["x"] + s["w"] // 2, s["y"] + s["h"] * 3 // 4
    tx, ty = t["x"] + t["w"] // 2, t["y"] + t["h"] // 2
    input_(f"motion {sx} {sy}")
    input_("button left down")
    steps = 20
    for i in range(1, steps + 1):
        input_(f"motion {sx + (tx - sx) * i // steps} {sy + (ty - sy) * i // steps}")
    # Settling on the target, as a hand does.
    input_(f"motion {tx + 2} {ty + 1}")
    input_(f"motion {tx} {ty}")
    time.sleep(0.5)
    input_("button left up")


def printed(name):
    # The target exits after the drop (--and-exit), which flushes its output.
    return FILE in machine.succeed(f"cat /tmp/{name}.out || true")


def copied():
    return machine.execute(f"test -f {FOLDER}/dropped.txt")[0] == 0


def close(*windows_):
    open_ = {w["id"] for w in windows()}
    for w in windows_:
        if w["id"] in open_:
            user(f"aizomectl close {w['id']}")
    ids = {w["id"] for w in windows_}
    until("closed", lambda: not any(w["id"] in ids for w in windows()))


# Apart and away from the X screen's origin, where satellite puts every X11
# window: each target is right of and above its source.
SOURCE = (80, 520)
TARGET = (760, 140)

machine.start()
machine.wait_until_succeeds("pgrep -u alice -x aizome-shell", timeout=120)
machine.wait_until_succeeds("pgrep -u alice -f 'xwayland-satellite :'", timeout=60)
user(f"echo dragged > {FILE} && mkdir -p {FOLDER}")
try:
    with subtest("an X11 drag lands on another X11 window"):
        source = dragon("source-x11", "x11", FILE, *SOURCE)
        target = dragon("target-x11", "x11", "--target --print-path --and-exit", *TARGET)
        drag(source, target)
        until("the X11 target printed the file", lambda: printed("target-x11"), timeout=10)
        machine.screenshot("01-x11-to-x11")
        close(source, target)

    with subtest("a Wayland drag lands on an X11 window"):
        source = dragon("source-wayland", "wayland", FILE, *SOURCE)
        target = dragon("target-x11-again", "x11", "--target --print-path --and-exit", *TARGET)
        drag(source, target)
        until("the X11 target printed the file", lambda: printed("target-x11-again"), timeout=10)
        machine.screenshot("02-wayland-to-x11")
        close(source, target)

    with subtest("an X11 drag lands on a Wayland window"):
        source = dragon("source-x11-again", "x11", FILE, *SOURCE)
        target = spawn("hako", f"aizome-hako {FOLDER}", *TARGET, 500, 360)
        drag(source, target)
        until("Hako copied the file", copied, timeout=10)
        machine.screenshot("03-x11-to-wayland")
        close(source, target)

    machine.fail("coredumpctl --no-pager list xwayland-satellite")
finally:
    machine.screenshot("final")
    Path(machine.out_dir, "inputs.json").write_text(json.dumps(inputs, indent=2))
    machine.succeed("journalctl _UID=1000 --no-pager > /tmp/xwayland-dnd.log")
    machine.copy_from_machine("/tmp/xwayland-dnd.log")
    machine.shutdown()
