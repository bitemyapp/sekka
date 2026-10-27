"""Packaged Tansu: actual input on its --sample in-memory disks.

This tests editable-size validation, not UDisks or an ISO installation. No app
messages or config changes are injected; inspection only reads painted controls.
"""
import json
import shlex
import time
from pathlib import Path

INSPECT = "/run/user/1000/tansu-size-inspect"
APP = "org.aizome.Tansu"
inputs = []


def user(command):
    return machine.succeed("su - alice -c " + shlex.quote(
        "export XDG_RUNTIME_DIR=/run/user/1000; "
        "export WAYLAND_DISPLAY=$(ls /run/user/1000 | grep -m1 '^wayland-[0-9]*$'); "
        "export AIZOME_SOCKET=/run/user/1000/aizome.$WAYLAND_DISPLAY.sock; " + command))


def windows():
    return json.loads(user("aizomectl --json windows"))["windows"]


def window(app=APP):
    return next((w for w in windows() if w["app_id"] == app), None)


def until(description, check):
    deadline = time.monotonic() + 20
    while time.monotonic() < deadline:
        value = check()
        if value:
            return value
        time.sleep(0.2)
    raise AssertionError(description)


def elements():
    w = window()
    if not w:
        return []
    docs = user(f"for f in {INSPECT}/{APP}.{w['pid']}.*.json; do "
                'if test -f "$f"; then cat "$f"; printf "\\n"; fi; done').splitlines()
    return [e for line in docs for d in [json.loads(line)] if d["kind"] == "window"
            for e in d["elements"] if e["visible"]]


def find(**attributes):
    return next((e for e in elements() if all(e.get(k) == v for k, v in attributes.items())), None)


def input_(command):
    inputs.append(command)
    user("aizomectl input " + command)
    time.sleep(0.2)


def click_element(e):
    g = window()["geometry"]
    x, y, width, height = e["rect"]
    input_(f"motion {g['x'] + x + width/2} {g['y'] + y + height/2}")
    input_("click left")


def click(**attributes):
    click_element(until(str(attributes), lambda: find(**attributes)))


def field(name, value):
    click(name=name)
    input_("chord super+a")
    if value:
        input_("type " + shlex.quote(value))
    else:
        input_("key BackSpace")
    until("field value " + value, lambda: find(name=name).get("value") == value)


def unchanged_plan():
    return any(e.get("text", "").startswith("No pending operations") for e in elements())


machine.start()
machine.wait_until_succeeds("pgrep -u alice -x aizome-shell", timeout=120)
try:
    tour = until("first-run tour", lambda: window("org.aizome.Annai"))
    g = tour["geometry"]
    input_(f"motion {g['x'] + 30} {g['y'] - 12}")
    input_("click left")
    input_("chord super+q")
    until("tour closed", lambda: window("org.aizome.Annai") is None)
    user(f"aizomectl spawn env AIZOME_KIT_INSPECT={INSPECT} aizome-tansu --sample")
    until("Tansu", window)
    row = until("sample Crucial drive", lambda: next(
        (e for e in elements() if e.get("label", "").startswith("Crucial CT500")), None))
    click_element(row)
    click(name="partition-map")
    input_("key End")
    click(role="button", label="New")
    until("create sheet", lambda: find(name="create-size"))
    with subtest("invalid create input blocks both pointer and Return, then recovers"):
        field("create-size", "1.02.1")
        input_("key Return")
        machine.screenshot("01-invalid-create-return")
        until("invalid create retained", lambda: find(name="create-size"))
        confirm = find(role="button", text="Add to Pending")
        assert confirm and confirm["disabled"]
        click_element(confirm)
        assert find(name="create-size") and unchanged_plan()
        for value in ["", "0", "999999999999999999999999999999999999999"]:
            field("create-size", value)
            input_("key Return")
            assert find(name="create-size") and unchanged_plan()
        machine.screenshot("02-create-overflow-rejected")
        field("create-size", "5 GB")
        click(role="button", text="Add to Pending")
        until("valid create queued", lambda: find(name="create-size") is None and not unchanged_plan())
        machine.screenshot("03-valid-create-queued")
        click(role="button", text="Undo")
        until("create undone", unchanged_plan)

    with subtest("invalid resize cannot use a valid prefix; cancel and corrected input work"):
        click(name="partition-map")
        input_("key End")
        input_("key Left")
        click(role="button", label="Unmount")
        until("sample volume unmounted", lambda: find(role="button", label="Mount"))
        click(role="button", label="Resize")
        field("resize-size", "120 GB")
        input_("type x")
        input_("key Return")
        machine.screenshot("04-invalid-resize-return")
        until("invalid resize retained", lambda: find(name="resize-size"))
        assert unchanged_plan()
        assert find(role="button", text="Shrink")["disabled"]
        click(role="button", text="Cancel")
        until("resize cancelled", lambda: find(name="resize-size") is None)
        assert unchanged_plan()
        click(role="button", label="Resize")
        field("resize-size", "120 GB")
        click(role="button", text="Shrink")
        until("valid resize queued", lambda: find(name="resize-size") is None and not unchanged_plan())
        machine.screenshot("05-valid-resize-queued")
        click(role="button", text="Undo")
        until("resize undone", unchanged_plan)
    input_("chord super+q")
    until("Tansu closed normally", lambda: window() is None)
    machine.fail("coredumpctl --no-pager list aizome-tansu")
finally:
    machine.screenshot("final")
    Path(machine.out_dir, "inputs.json").write_text(json.dumps(inputs, indent=2))
    machine.succeed("journalctl _UID=1000 --no-pager > /tmp/tansu-size.log")
    machine.copy_from_machine("/tmp/tansu-size.log")
    machine.copy_from_machine(INSPECT)
    machine.shutdown()
