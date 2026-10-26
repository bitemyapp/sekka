"""Component VM: real input, inspected paint locations, no app-message injection."""

import json
import shlex
import time

APP = "org.aizome.Shashin"
INSPECT = "/run/user/1000/panel-inspect"
PICTURES = "/home/alice/Pictures"


def user(command):
    return machine.succeed(
        "su - alice -c "
        + shlex.quote(
            "export XDG_RUNTIME_DIR=/run/user/1000; "
            "export WAYLAND_DISPLAY=$(ls /run/user/1000 | grep -m1 '^wayland-[0-9]*$'); "
            "export AIZOME_SOCKET=/run/user/1000/aizome.$WAYLAND_DISPLAY.sock; "
            + command
        )
    )


def window():
    return next(
        (
            w
            for w in json.loads(user("aizomectl --json windows"))["windows"]
            if w["app_id"] == APP
        ),
        None,
    )


def elements():
    current = window()
    if current is None:
        return []
    docs = user(
        f"for file in {INSPECT}/{APP}.{current['pid']}.*.json; do "
        'if test -f "$file"; then cat "$file"; printf "\\n"; fi; done'
    ).splitlines()
    return next(
        (json.loads(d)["elements"] for d in docs if json.loads(d)["kind"] == "window"),
        [],
    )


def find(**attributes):
    return next(
        (
            e
            for e in elements()
            if e["visible"] and all(e.get(k) == v for k, v in attributes.items())
        ),
        None,
    )


def until(description, check):
    deadline = time.monotonic() + 15
    while time.monotonic() < deadline:
        result = check()
        if result:
            return result
        time.sleep(0.2)
    machine.screenshot("failed")
    raise AssertionError(description)


def click(**attributes):
    element = until(str(attributes), lambda: find(**attributes))
    x, y, width, height = element["rect"]
    geometry = window()["geometry"]
    user(
        f"aizomectl input motion {geometry['x'] + x + width / 2} {geometry['y'] + y + height / 2}"
    )
    user("aizomectl input click")
    time.sleep(0.3)


def chord(keys):
    user("aizomectl input chord " + keys)
    time.sleep(0.3)


def key(name):
    user("aizomectl input key " + name)
    time.sleep(0.3)


def count(label):
    return any(e["visible"] and label in e.get("text", "") for e in elements())


def choose_range():
    chord("super+o")
    until("open panel", lambda: find(text="Open a Picture"))
    click(text="first.png")
    chord("shift+Down")


machine.start()
machine.wait_until_succeeds("pgrep -u alice -x aizome-shell", timeout=120)
machine.succeed("install -d -o alice -g users " + PICTURES)
machine.succeed(
    "install -d -o alice -g users " + PICTURES + "/archive " + PICTURES + "/projects"
)
machine.succeed("install -o alice -g users /etc/panel-pictures/*.png " + PICTURES)
original = machine.succeed("sha256sum " + PICTURES + "/*.png")
user("mkdir -p " + INSPECT)
user(
    f"aizomectl spawn env AIZOME_KIT_INSPECT={INSPECT} aizome-shashin {PICTURES}/first.png"
)
until("initial three-picture folder", lambda: count("1 of 3"))
machine.screenshot("01-folder")

with subtest("mixed selection refuses folders and recovers"):
    chord("super+o")
    until("open panel", lambda: find(text="Open a Picture"))
    click(text="projects")
    chord("shift+Down")
    until(
        "mixed selection guidance",
        lambda: find(text="Select files, or open one folder."),
    )
    machine.screenshot("mixed-selection")
    key("Return")
    assert find(text="Open a Picture") is not None
    click(text="Open")
    assert find(text="Open a Picture") is not None
    click(text="first.png")
    assert find(text="Select files, or open one folder.") is None
    key("Escape")
    until("mixed selection cancelled", lambda: find(text="Open a Picture") is None)
    assert count("1 of 3")

with subtest("cancel preserves the original folder"):
    choose_range()
    key("Escape")
    until("cancel closes panel", lambda: find(text="Open a Picture") is None)
    assert count("1 of 3")
    machine.screenshot("02-cancelled")

for method in ["button", "return"]:
    with subtest("two selected files via " + method):
        choose_range()
        machine.screenshot(method + "-selected")
        if method == "button":
            click(text="Open")
        else:
            key("Return")
        until("panel closed", lambda: find(text="Open a Picture") is None)
        machine.screenshot(method + "-opened")
        assert count("1 of 2"), "Open must keep exactly the two selected pictures"
        click(name="stage")
        key("Right")
        until(
            "second selected picture",
            lambda: count("2 of 2") and "second.png" in window()["title"],
        )
        machine.screenshot(method + "-second")
        assert machine.succeed("sha256sum " + PICTURES + "/*.png") == original

with subtest("Return ignores a cursor row toggled off with Command-click"):
    choose_range()
    user("aizomectl input key super down")
    click(text="second.png")
    user("aizomectl input key super up")
    machine.screenshot("toggle-selected")
    key("Return")
    until(
        "remaining first file",
        lambda: (
            find(text="Open a Picture") is None and "first.png" in window()["title"]
        ),
    )
    until("single-file folder navigation", lambda: count("1 of 3"))
    machine.screenshot("toggle-opened")
    chord("super+o")
    until("open panel", lambda: find(text="Open a Picture"))
    click(text="first.png")
    user("aizomectl input key super down")
    click(text="first.png")
    user("aizomectl input key super up")
    key("Return")
    assert find(text="Open a Picture") is not None
    machine.screenshot("toggle-empty")
    key("Escape")
    until("empty selection cancelled", lambda: find(text="Open a Picture") is None)

chord("super+q")
until("normal quit", lambda: window() is None)
machine.fail("coredumpctl --no-pager list aizome-shashin")
