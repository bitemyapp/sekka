"""Compare actual Sumi search pixels with a normal full-redraw control.

All search changes use pointer/keyboard input. Inspection only locates controls
and reads visible counts. Fill then restore requests an ordinary full redraw at
the same geometry; it is a comparison control, not a workaround used to pass.
"""

import json
import shlex
import time
from pathlib import Path

from PIL import Image, ImageChops

APP = "org.aizome.Sumi"
INSPECT = "/run/user/1000/sumi-inspect"
inputs = []
observations = []


def user(command):
    return machine.succeed("su - alice -c " + shlex.quote(
        "export XDG_RUNTIME_DIR=/run/user/1000; "
        "export WAYLAND_DISPLAY=$(ls /run/user/1000 | grep -m1 '^wayland-[0-9]*$'); "
        "export AIZOME_SOCKET=/run/user/1000/aizome.$WAYLAND_DISPLAY.sock; "
        + command))


def until(description, check):
    deadline = time.monotonic() + 20
    while time.monotonic() < deadline:
        value = check()
        if value:
            return value
        time.sleep(0.2)
    raise AssertionError(description)


def window(app=APP):
    return next((w for w in json.loads(user("aizomectl --json windows"))["windows"]
                 if w["app_id"] == app), None)


def elements():
    w = window()
    docs = [json.loads(line) for line in machine.succeed(
        f"for f in {INSPECT}/{APP}.*.json; do if test -f \"$f\"; then cat \"$f\"; printf '\\n'; fi; done"
    ).splitlines() if line.strip()]
    return [e for d in docs if w and d.get("pid") == w["pid"] and d.get("kind") == "window"
            for e in d["elements"] if e["visible"]]


def input_(command):
    inputs.append(command)
    user("aizomectl input " + command)
    time.sleep(0.2)


def click(x, y):
    input_(f"motion {x} {y}")
    input_("click left")


def click_element(e):
    g = window()["geometry"]
    x, y, width, height = e["rect"]
    click(g["x"] + x + width / 2, g["y"] + y + height / 2)


def option(label):
    e = until(label, lambda: next((e for e in elements()
                                 if e.get("role") == "button" and e.get("label") == label), None))
    click_element(e)
    # Remove the hover tooltip without changing document focus/selection.
    input_("motion 1000 12")


def summary(text):
    until("visible search status " + text, lambda: any(e.get("text") == text for e in elements()))


def query(text):
    e = until("Find field", lambda: next((e for e in elements() if e.get("name") == "find"), None))
    click_element(e)
    input_("chord super+a")
    input_("type " + shlex.quote(text))
    input_("motion 1000 12")


def assert_current_pixels(name):
    g = window()["geometry"]
    e = next(e for e in elements() if e.get("name") == "editor")
    x, y, width, height = e["rect"]
    box = tuple(round(v) for v in (g["x"] + x, g["y"] + y,
                                   g["x"] + x + width, g["y"] + y + height))
    time.sleep(0.3)
    machine.screenshot(name + "-incremental")
    before = Image.open(Path(machine.out_dir, name + "-incremental.png")).convert("RGB").crop(box)
    input_("chord alt+super+Up")
    until("Fill control", lambda: window()["fill"])
    input_("chord alt+super+Up")
    until("original geometry restored", lambda: not window()["fill"] and window()["geometry"] == g)
    time.sleep(0.3)
    machine.screenshot(name + "-full-redraw")
    after = Image.open(Path(machine.out_dir, name + "-full-redraw.png")).convert("RGB").crop(box)
    changed = ImageChops.difference(before, after).getbbox()
    observations.append({"case": name, "geometry": g, "editor_box": box, "difference_bbox": changed})
    assert changed is None, name + ": search marks stayed stale until full redraw: " + repr(changed)


machine.start()
machine.wait_until_succeeds("pgrep -u alice -x aizome-shell", timeout=120)
try:
    tour = until("first-run tour", lambda: window("org.aizome.Annai"))
    g = tour["geometry"]
    click(g["x"] + 35, g["y"] - 12)
    input_("chord super+q")
    until("tour closed", lambda: window("org.aizome.Annai") is None)
    user("aizomectl spawn aizome-sumi /etc/sumi-search.txt")
    g = until("Sumi document", window)["geometry"]
    click(g["x"] + 35, g["y"] - 12)
    input_("chord super+f")
    query("cat")
    summary("1 of 5")
    machine.screenshot("01-five-matches")

    with subtest("whole-word and case options repaint without moving the selection"):
        option("Whole Words")
        summary("1 of 2")
        assert_current_pixels("02-whole-words")
        option("Match Case")
        summary("1 of 1")
        assert_current_pixels("03-case-sensitive")

    with subtest("no matches and invalid regex remove old highlights"):
        query("zzzzz")
        summary("No matches")
        assert_current_pixels("04-no-matches")
        option("Whole Words")
        option("Match Case")
        query("cat")
        summary("1 of 5")
        option("Regular Expression")
        query("[")
        summary("unclosed character class")
        assert_current_pixels("05-invalid-regex")

    with subtest("a new search moves marks to another line and navigation still works"):
        query("number")
        summary("1 of 2")
        assert_current_pixels("06-second-line")
        input_("chord super+g")
        summary("2 of 2")
        input_("chord shift+super+g")
        summary("1 of 2")
        assert_current_pixels("07-previous-match")
        assert machine.succeed("cat /etc/sumi-search.txt") == (
            "cat Cat caterpillar cat1 cat2\nnumber:12 number:34\nplain line\n")
        input_("chord super+q")
        until("normal Sumi Quit", lambda: window() is None)
        machine.fail("coredumpctl --no-pager list aizome-sumi")
finally:
    machine.screenshot("final")
    Path(machine.out_dir, "inputs.json").write_text(json.dumps(inputs, indent=2))
    Path(machine.out_dir, "observations.json").write_text(json.dumps(observations, indent=2))
    machine.succeed("journalctl _UID=1000 --no-pager > /tmp/sumi-search.log")
    machine.copy_from_machine("/tmp/sumi-search.log")
    machine.copy_from_machine(INSPECT)
    machine.shutdown()
