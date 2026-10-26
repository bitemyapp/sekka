"""Real pointer/keyboard regression; inspection only reads painted controls.

Standard Settings is omitted and role-only Help remains unavailable,
while Koyomi and Tenki's actual message-backed Settings still open and close.
This component fixture does not qualify an ISO or physical hardware.
"""
import json
import shlex
import time
from pathlib import Path

INSPECT = "/run/user/1000/menu-inspect"
inputs = []


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


def window(name):
    return next((w for w in json.loads(user("aizomectl --json windows"))["windows"]
                 if w["app_id"] == "org.aizome." + name), None)


def documents():
    return [json.loads(line) for line in machine.succeed(
        f"for f in {INSPECT}/*.json; do if test -f \"$f\"; then cat \"$f\"; printf '\\n'; fi; done"
    ).splitlines() if line.strip()]


def elements(name):
    w = window(name)
    if not w:
        return []
    return [e for d in documents() if d.get("app_id") == w["app_id"] and d.get("pid") == w["pid"]
            and d.get("kind") == "window" for e in d["elements"] if e["visible"]]


def text_visible(name, label):
    return any(e.get("text") == label for e in elements(name))


def input_(command):
    inputs.append(command)
    user("aizomectl input " + command)
    time.sleep(0.15)


def click(x, y):
    input_(f"motion {x} {y}")
    input_("click left")


def shell_state():
    return json.loads(user("aizome-shell msg state"))


def menu(label):
    # The bar exposes painted label bounds, not a test-only activation API.
    def find():
        return next((e for d in documents() if d.get("title") == "Menu bar"
                     for e in d["elements"] if e["visible"]
                     and e.get("role") == "menuitem" and e.get("label") == label), None)
    e = until("menu title " + label, find)
    x, y, width, height = e["rect"]
    click(x + width / 2, y + height / 2)
    until("menu opens", lambda: shell_state()["menu"] is not None)
    return x, y + height


def close_menu():
    input_("key Escape")
    until("menu closes", lambda: shell_state()["menu"] is None)


def launch(name):
    user(f"aizomectl spawn env AIZOME_KIT_INSPECT={INSPECT} aizome-{name.lower()}")
    w = until(name + " window", lambda: window(name))
    g = w["geometry"]
    click(g["x"] + g["w"] / 2, g["y"] + 8)
    until(name + " painted", lambda: elements(name))


def done(name):
    e = until(name + " Done", lambda: next(
        (e for e in elements(name) if e.get("text") == "Done" and e.get("role") == "button"), None))
    x, y, width, height = e["rect"]
    g = window(name)["geometry"]
    click(g["x"] + x + width / 2, g["y"] + y + height / 2)
    until(name + " Settings closed", lambda: not text_visible(name, name + " Settings"))


machine.start()
machine.wait_until_succeeds("pgrep -u alice -x aizome-shell", timeout=120)
try:
    with subtest("unsupported standard actions stay unavailable and menus recover"):
        launch("Shashin")
        x, bottom = menu("Shashin")
        # Settings is omitted; the captured menu verifies the visible result.
        # Unit tests also assert omission from native and published menus.
        machine.screenshot("01-shashin-settings-omitted")
        close_menu()
        input_("chord super+comma")
        assert not text_visible("Shashin", "Settings")
        assert window("Shashin") is not None
        x, bottom = menu("Help")
        machine.screenshot("02-shashin-help-unavailable")
        # Help contains Search followed by the app-specific Help entry.
        click(x + 40, bottom + 40)
        assert shell_state()["menu"] is not None, "disabled Help must not activate"
        close_menu()
        x, bottom = menu("Shashin")
        click(x + 40, bottom + 16)
        until("About still opens", lambda: any("Part of Aizome" in e.get("text", "") for e in elements("Shashin")))
        machine.screenshot("03-shashin-about-works")
        input_("key Return")
        input_("chord super+q")
        until("Shashin quits", lambda: window("Shashin") is None)

    for number, name in enumerate(["Koyomi", "Tenki"], 4):
        with subtest(name + " keeps its real Settings action and shortcut"):
            launch(name)
            x, bottom = menu(name)
            machine.screenshot(f"{number:02}-{name.lower()}-settings-enabled")
            click(x + 40, bottom + 48)
            until(name + " Settings pointer", lambda: text_visible(name, name + " Settings"))
            machine.screenshot(f"{number:02}-{name.lower()}-settings-pointer")
            done(name)
            input_("chord super+comma")
            until(name + " Settings shortcut", lambda: text_visible(name, name + " Settings"))
            machine.screenshot(f"{number:02}-{name.lower()}-settings-shortcut")
            done(name)
            input_("chord super+q")
            until(name + " quits", lambda: window(name) is None)
    for app in ["shashin", "koyomi", "tenki"]:
        machine.fail("coredumpctl --no-pager list aizome-" + app)
finally:
    machine.screenshot("final")
    Path(machine.out_dir, "inputs.json").write_text(json.dumps(inputs, indent=2))
    machine.succeed("journalctl _UID=1000 --no-pager > /tmp/standard-menu.log")
    machine.copy_from_machine("/tmp/standard-menu.log")
    machine.copy_from_machine(INSPECT)
    machine.shutdown()
