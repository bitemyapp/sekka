"""Inspect visible controls; send ordinary compositor pointer/keyboard input.

This component test seeds one disposable calendar. It does not qualify an ISO.
"""

import json
import shlex
import time

APP = "org.aizome.Koyomi"
INSPECT = "/run/user/1000/modal-inspect"
CALENDAR = "/home/alice/.local/share/aizome/koyomi/modal.ics"


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


def saved():
    return machine.succeed("cat " + CALENDAR)


def no_editor():
    until("editor closed", lambda: find(name="edit-title") is None)


def edit(title):
    # One occurrence occupies this day's strip. The agenda text is only a label.
    click(name="all-day")
    chord("super+i")
    until("event editor", lambda: find(name="edit-title"))
    assert find(name="edit-title")["value"] == title


machine.start()
machine.wait_until_succeeds("pgrep -u alice -x aizome-shell", timeout=120)
machine.succeed("install -d -o alice -g users /home/alice/.local/share/aizome/koyomi")
machine.succeed("install -o alice -g users /etc/koyomi-modal-fixture.ics " + CALENDAR)
user("mkdir -p " + INSPECT)
user(
    f"aizomectl spawn env AIZOME_KIT_INSPECT={INSPECT} aizome-koyomi --show 2026-10-23"
)
until("Koyomi window", window)
machine.wait_until_succeeds(f'test -n "$(ls {INSPECT}/{APP}.*.json)"')
until("recurring event", lambda: find(text="Three day test"))
machine.screenshot("01-recurring-event")

with subtest("occurrence confirmation cancels above editor and restores draft focus"):
    edit("Three day test")
    click(name="edit-title")
    chord("super+a")
    type_text("Second occurrence")
    click(name="edit-save")
    until(
        "save confirmation", lambda: find(text="Save for this event, or all of them?")
    )
    machine.screenshot("02a-save-confirmation")
    type_text("leaked")
    key("Escape")
    until(
        "alert cancelled",
        lambda: find(text="Save for this event, or all of them?") is None,
    )
    assert find(name="edit-title")["focused"]
    assert find(name="edit-title")["value"] == "Second occurrence"
    assert "SUMMARY:Three day test" in saved()
    assert "RECURRENCE-ID" not in saved()
    machine.screenshot("02-cancel-keeps-editor")

with subtest("pointer saves only the selected occurrence and survives reopen"):
    click(name="edit-save")
    click(text="Only This Event")
    no_editor()
    until("exception saved", lambda: "SUMMARY:Second occurrence" in saved())
    assert "SUMMARY:Three day test" in saved()
    assert "RECURRENCE-ID;VALUE=DATE:20261023" in saved()
    machine.screenshot("03-occurrence-saved")
    chord("super+q")
    until("Koyomi quit", lambda: window() is None)
    user(
        f"aizomectl spawn env AIZOME_KIT_INSPECT={INSPECT} aizome-koyomi --show 2026-10-23"
    )
    until("reopened occurrence", lambda: window() and find(text="Second occurrence"))

with subtest("delete confirmation cancels then deletes only this occurrence"):
    edit("Second occurrence")
    click(text="Delete…")
    until(
        "delete confirmation",
        lambda: find(text="Delete this event, or more of the series?"),
    )
    machine.screenshot("04a-delete-confirmation")
    key("Escape")
    until(
        "delete cancelled",
        lambda: find(text="Delete this event, or more of the series?") is None,
    )
    # Koyomi intentionally closes its editor before asking about deletion.
    # Cancelling preserves the stored occurrence, which can be edited again.
    no_editor()
    assert "SUMMARY:Second occurrence" in saved()
    edit("Second occurrence")
    click(text="Delete…")
    click(text="Only This Event")
    no_editor()
    until("occurrence excluded", lambda: "EXDATE;VALUE=DATE:20261023" in saved())
    assert "SUMMARY:Three day test" in saved()
    assert "SUMMARY:Second occurrence" not in saved()
    machine.screenshot("04-occurrence-deleted")

machine.copy_from_machine(CALENDAR)
machine.succeed("journalctl _UID=1000 --no-pager > /tmp/koyomi-modal.log")
machine.copy_from_machine("/tmp/koyomi-modal.log")
machine.fail("coredumpctl --no-pager list aizome-koyomi")
