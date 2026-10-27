"""Real pointer/keyboard component regression, not an ISO qualification.

Inspection only locates painted controls. No application message injection.
"""

import json
import shlex
import time

APP = "org.aizome.Shashin"
INSPECT = "/run/user/1000/modal-inspect"
PICTURE = "/home/alice/Pictures/modal.jpg"


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


def ipc(command):
    return json.loads(user("aizomectl --json " + command))


def until(description, check):
    deadline = time.monotonic() + 20
    while time.monotonic() < deadline:
        result = check()
        if result:
            return result
        time.sleep(0.3)
    machine.screenshot("failed")
    raise AssertionError(description)


def window():
    return next((w for w in ipc("windows")["windows"] if w["app_id"] == APP), None)


def document():
    current = window()
    if current is None:
        return {"elements": []}
    # Quit may leave an old inspection file. Read only the live window's
    # process, and wait for its first painted document after reopening.
    docs = user(
        f"for file in {INSPECT}/{APP}.{current['pid']}.*.json; do "
        'if test -f "$file"; then cat "$file"; printf "\\n"; fi; done'
    ).splitlines()
    return next(
        (json.loads(d) for d in docs if json.loads(d)["kind"] == "window"),
        {"elements": []},
    )


def find(**attributes):
    return next(
        (
            e
            for e in document()["elements"]
            if e["visible"] and all(e.get(k) == v for k, v in attributes.items())
        ),
        None,
    )


def click(**attributes):
    element = until(str(attributes), lambda: find(**attributes))
    x, y, width, height = element["rect"]
    geometry = window()["geometry"]
    user(
        f"aizomectl input motion {geometry['x'] + x + width / 2} "
        f"{geometry['y'] + y + height / 2}"
    )
    user("aizomectl input click")
    time.sleep(0.3)


def key(name):
    user("aizomectl input key " + name)
    time.sleep(0.3)


def chord(keys):
    user("aizomectl input chord " + keys)
    time.sleep(0.3)


def type_text(text):
    user("aizomectl input type " + shlex.quote(text))
    time.sleep(0.3)


def digest():
    return machine.succeed("sha256sum " + PICTURE).split()[0]


def alert():
    return find(text="Save a turned picture?")


machine.start()
machine.wait_until_succeeds("pgrep -u alice -x aizome-shell", timeout=120)
machine.succeed("install -d -o alice -g users /home/alice/Pictures")
machine.succeed("install -o alice -g users /etc/shashin-modal.jpg " + PICTURE)
user("mkdir -p " + INSPECT)
user(f"aizomectl spawn env AIZOME_KIT_INSPECT={INSPECT} aizome-shashin {PICTURE}")
until("Shashin window", window)
until("picture displayed", lambda: find(name="stage"))
original = digest()
machine.screenshot("01-picture")

with subtest("Escape cancels without saving and preserves rotated draft"):
    click(name="stage")
    chord("super+r")
    until("rotated draft", lambda: "Turned" in window()["title"])
    chord("super+q")
    until("unsaved alert", alert)
    machine.screenshot("02-unsaved-alert")
    key("Escape")
    until("alert cancelled", lambda: alert() is None)
    assert "Turned" in window()["title"]
    assert digest() == original
    machine.screenshot("03-escape-retains-draft")

with subtest("application shortcuts cannot save or undo behind the alert"):
    chord("super+q")
    until("unsaved alert again", alert)
    chord("super+s")
    machine.screenshot("04a-save-blocked")
    current = digest()
    print(f"JPEG before={original}, after blocked Save={current}")
    assert current == original, "Save behind the alert changed the picture"
    for command in ["super+z", "super+w", "super+q"]:
        chord(command)
    machine.screenshot("04-commands-blocked")
    assert alert() is not None
    assert "Turned" in window()["title"]
    assert digest() == original, "Save behind the alert changed the picture"
    # Normal shell menu interaction; File > Save must be visibly unavailable.
    user("aizomectl input motion 180 12")
    user("aizomectl input click")
    time.sleep(0.4)
    machine.screenshot("05-file-menu-over-alert")
    key("Escape")
    click(text="Cancel")
    until("pointer cancel", lambda: alert() is None)
    assert digest() == original

with subtest("default Save commits and quits, then normal commands recover"):
    chord("super+q")
    until("final unsaved alert", alert)
    key("Return")
    until("saved and quit", lambda: window() is None)
    assert digest() != original
    user(f"aizomectl spawn env AIZOME_KIT_INSPECT={INSPECT} aizome-shashin {PICTURE}")
    until("reopened saved picture", lambda: window() and find(name="stage"))
    assert "Turned" not in window()["title"]
    machine.screenshot("06-reopened-saved")
    chord("super+r")
    until("next draft", lambda: "Turned" in window()["title"])
    chord("super+q")
    until("discard confirmation", alert)
    click(text="Don't Save")
    until("discarded and quit", lambda: window() is None)

machine.copy_from_machine(PICTURE)
machine.succeed("journalctl _UID=1000 --no-pager > /tmp/shashin-modal.log")
machine.copy_from_machine("/tmp/shashin-modal.log")
machine.fail("coredumpctl --no-pager list aizome-shashin")
