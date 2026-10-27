"""Real Tokei pointer/keyboard input; IPC/inspection only read visible results.

No city configuration or application messages are injected. The default cities
are reordered, persisted through normal Quit, and compared with the compositor's
unchanged Fill/Minimise shortcuts. This does not qualify a candidate ISO.
"""

import json
import shlex
import time
from pathlib import Path

INSPECT = "/run/user/1000/tokei-inspect"
inputs = []
observations = []


def user(command):
    return machine.succeed("su - alice -c " + shlex.quote(
        "export XDG_RUNTIME_DIR=/run/user/1000; "
        "export WAYLAND_DISPLAY=$(ls /run/user/1000 | grep -m1 '^wayland-[0-9]*$'); "
        "export AIZOME_SOCKET=/run/user/1000/aizome.$WAYLAND_DISPLAY.sock; "
        "export AIZOME_SHELL_SOCKET=/run/user/1000/aizome-shell.$WAYLAND_DISPLAY.sock; "
        + command))


def until(description, check):
    deadline = time.monotonic() + 20
    while time.monotonic() < deadline:
        value = check()
        if value:
            return value
        time.sleep(0.2)
    raise AssertionError(description)


def window():
    return next((w for w in json.loads(user("aizomectl --json windows"))["windows"]
                 if w["app_id"] == "org.aizome.Tokei"), None)


def documents():
    return [json.loads(line) for line in machine.succeed(
        f"for f in {INSPECT}/*.json; do if test -f \"$f\"; then cat \"$f\"; printf '\\n'; fi; done"
    ).splitlines() if line.strip()]


def elements():
    w = window()
    return [e for d in documents() if w and d.get("app_id") == w["app_id"]
            and d.get("pid") == w["pid"] and d.get("kind") == "window"
            for e in d["elements"] if e["visible"]]


def cities():
    return [e["text"] for e in sorted(elements(), key=lambda e: e["rect"][1])
            if e.get("text") in ["New York", "London", "Tokyo"]]


def input_(command):
    inputs.append(command)
    user("aizomectl input " + command)
    time.sleep(0.2)


def click(x, y):
    input_(f"motion {x} {y}")
    input_("click left")


def shell_state():
    return json.loads(user("aizome-shell msg state"))


def menu():
    e = until("Clock menu title", lambda: next(
        (e for d in documents() if d.get("title") == "Menu bar"
         for e in d["elements"] if e["visible"] and e.get("role") == "menuitem"
         and e.get("label") == "Clock"), None))
    x, y, width, height = e["rect"]
    click(x + width / 2, y + height / 2)
    until("Clock menu open", lambda: shell_state()["menu"] is not None)
    return x, y + height


def close_menu():
    input_("key Escape")
    until("Clock menu closed", lambda: shell_state()["menu"] is None)


def assert_menu_disabled(label, origin):
    # Shell menus draw their rows without per-item inspection. At the fixed
    # 1280x800 fixture, the captured Clock menu has two rows, a separator,
    # then Move Up and Move Down. A disabled pointer hit keeps it open.
    left, bottom = origin
    click(left + 40, bottom + {"Move Up": 72, "Move Down": 96}[label])
    assert shell_state()["menu"] is not None, label + " should stay disabled"


def assert_order(expected, original_geometry=None):
    until("visible cities in order " + repr(expected), lambda: cities() == expected)
    w = window()
    assert not w["minimised"] and not w["hidden"], w
    if original_geometry is not None:
        assert w["geometry"] == original_geometry and not w["fill"], w
    observations.append({"cities": cities(), "window": w})


def launch():
    user(f"aizomectl spawn env AIZOME_KIT_INSPECT={INSPECT} aizome-tokei")
    w = until("Tokei window", window)
    g = w["geometry"]
    # Focus the compositor's title tab, above the content. Clicking the top
    # of Tokei's content would select a page in its segmented control.
    click(g["x"] + 35, g["y"] - 12)
    input_("chord super+1")
    until("three visible cities", lambda: len(cities()) == 3)


machine.start()
machine.wait_until_succeeds("pgrep -u alice -x aizome-shell", timeout=120)
try:
    with subtest("both city directions and boundaries leave the window unchanged"):
        launch()
        geometry = window()["geometry"]
        assert_order(["New York", "London", "Tokyo"], geometry)
        e = next(e for e in elements() if e.get("text") == "London")
        x, y, width, height = e["rect"]
        click(geometry["x"] + x + width / 2, geometry["y"] + y + height / 2)
        menu()
        machine.screenshot("01-clock-menu-shortcuts")
        close_menu()
        input_("chord shift+super+Up")
        assert_order(["London", "New York", "Tokyo"], geometry)
        origin = menu()
        assert_menu_disabled("Move Up", origin)
        machine.screenshot("02-london-first-up-disabled")
        close_menu()
        input_("chord shift+super+Up")
        assert_order(["London", "New York", "Tokyo"], geometry)
        input_("chord shift+super+Down")
        assert_order(["New York", "London", "Tokyo"], geometry)
        input_("chord shift+super+Down")
        assert_order(["New York", "Tokyo", "London"], geometry)
        origin = menu()
        assert_menu_disabled("Move Down", origin)
        machine.screenshot("03-london-last-down-disabled")
        close_menu()
        input_("chord shift+super+Down")
        assert_order(["New York", "Tokyo", "London"], geometry)

    with subtest("Alt+Super still fills and minimises without reordering cities"):
        input_("chord alt+super+Up")
        until("global Fill", lambda: window()["fill"] and window()["geometry"] != geometry)
        assert_order(["New York", "Tokyo", "London"])
        machine.screenshot("04-global-fill")
        input_("chord alt+super+Down")
        until("global Minimise", lambda: window()["minimised"])
        machine.screenshot("05-global-minimise")
        # Deskbar is anchored top-right, below the 24px menu bar. Its painted
        # app row is clicked in surface coordinates, never IPC-unminimised.
        d, e = until("Deskbar Tokei app row", lambda: next(
            ((d, e) for d in documents() if d.get("title") == "Deskbar"
             for e in d["elements"] if e["visible"] and e.get("text") == "Tokei"), None))
        x, y, width, height = e["rect"]
        click(1280 - d["size"][0] + x + width / 2, 24 + y + height / 2)
        until("Deskbar restores Tokei", lambda: not window()["minimised"])
        assert_order(["New York", "Tokyo", "London"])
        machine.screenshot("06-deskbar-restores")

    with subtest("normal Quit and relaunch preserve the changed order"):
        input_("chord super+q")
        until("Tokei Quit", lambda: window() is None)
        persisted = json.loads(user("cat ~/.local/state/aizome/tokei/state.json"))
        assert [p["name"] for p in persisted["places"]] == ["New York", "Tokyo", "London"], persisted
        launch()
        assert_order(["New York", "Tokyo", "London"])
        machine.screenshot("07-reopened-order-persists")
        input_("chord super+q")
        until("Tokei final Quit", lambda: window() is None)
    machine.fail("coredumpctl --no-pager list aizome-tokei")
finally:
    machine.screenshot("final")
    Path(machine.out_dir, "inputs.json").write_text(json.dumps(inputs, indent=2))
    Path(machine.out_dir, "observations.json").write_text(json.dumps(observations, indent=2))
    machine.succeed("journalctl _UID=1000 --no-pager > /tmp/tokei-reorder.log")
    machine.copy_from_machine("/tmp/tokei-reorder.log")
    machine.copy_from_machine(INSPECT)
    machine.shutdown()
