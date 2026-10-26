"""Actual packaged Fusen; filesystem permissions are explicit fault injection.

Inspector locates visible UI only. All edits, retries and exits use normal input.
No package/guest source modifications are made by the test.
"""

import json
import shlex
import time
from pathlib import Path

APP = "org.aizome.Fusen"
INSPECT = "/run/user/1000/fusen-inspect"
inputs = []


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


def windows(app=APP):
    return [
        w
        for w in json.loads(user("aizomectl --json windows"))["windows"]
        if w["app_id"] == app
    ]


def until(description, check):
    deadline = time.monotonic() + 20
    while time.monotonic() < deadline:
        value = check()
        if value:
            return value
        time.sleep(0.2)
    raise AssertionError(description)


def elements(w):
    docs = [
        json.loads(line)
        for line in machine.succeed(
            f'for f in {INSPECT}/{APP}.{w["pid"]}.*.json; do if test -f "$f"; then cat "$f"; printf \'\\n\'; fi; done'
        ).splitlines()
        if line.strip()
    ]
    # Window title maps main and sticky roots; alerts are inline overlays.
    return [
        e
        for d in docs
        if d.get("kind") in {"window", "utility"} and d.get("title") == w["title"]
        for e in d["elements"]
        if e["visible"]
    ]


def input_(command):
    inputs.append(command)
    user("aizomectl input " + command)
    time.sleep(0.2)


def click(x, y):
    input_(f"motion {x} {y}")
    input_("click left")


def find(w, **attrs):
    # Inline modal controls are painted last; a disabled banner behind an
    # alert can have the same Retry label and must not be clicked instead.
    matches = [e for e in elements(w) if all(e.get(k) == v for k, v in attrs.items())]
    return matches[-1] if matches else None


def button(w, label):
    e = until("button " + label, lambda: find(w, role="button", label=label))
    x, y, width, height = e["shown"]
    g = w["geometry"]
    click(g["x"] + x + width / 2, g["y"] + y + height / 2)


def focus(w):
    g = w["geometry"]
    click(g["x"] + 30, g["y"] - 12)


def has_error(w):
    return any(
        ("couldn't be saved" in e.get("text", "") or e.get("text") == "Not saved")
        for e in elements(w)
    )


def text(s):
    input_("type " + shlex.quote(s))


def chord(s):
    input_("chord " + s)


def read_note():
    return user("cat ~/Notes/QA/Draft.md")


def launch():
    user("aizomectl spawn aizome-fusen")
    w = until("Fusen window", lambda: next(iter(windows()), None))
    until("editor ready", lambda: find(w, name="search"))
    focus(w)
    return w


machine.start()
machine.wait_until_succeeds("pgrep -u alice -x aizome-shell", timeout=120)
try:
    tour = until("tour", lambda: next(iter(windows("org.aizome.Annai")), None))
    focus(tour)
    chord("super+q")
    until("tour closed", lambda: not windows("org.aizome.Annai"))
    w = launch()
    # Create and select the disposable folder through the normal New Folder UI.
    chord("shift+super+n")
    text("QA")
    input_("key Return")
    user("test -d ~/Notes/QA && test -w ~/Notes/QA")
    user("chmod 500 ~/Notes/QA")
    chord("super+n")
    text("Draft")
    input_("key Return")
    text("Keep these exact bytes.")
    time.sleep(1)
    machine.screenshot("01-failed-autosave")
    assert has_error(w), "unwritable note must show a save error"
    machine.fail("test -f /home/alice/Notes/QA/Draft.md")

    with subtest("normal Close and Quit preserve the draft and allow cancellation"):
        chord("super+w")
        until(
            "Keep Open after Close", lambda: find(w, role="button", label="Keep Open")
        )
        assert windows()
        machine.screenshot("02-close-protected")
        input_("key Escape")
        until(
            "close cancelled", lambda: find(w, role="button", label="Keep Open") is None
        )
        chord("super+q")
        until("Keep Open after Quit", lambda: find(w, role="button", label="Keep Open"))
        machine.screenshot("03-quit-protected")
        button(w, "Keep Open")
        assert windows()

    with subtest(
        "permission restoration retries exact draft and survives normal reopen"
    ):
        user("chmod 700 ~/Notes/QA")
        button(w, "Retry")
        until("saved banner cleared", lambda: not has_error(w))
        assert read_note() == "Draft\nKeep these exact bytes."
        machine.screenshot("04-recovered-draft")
        chord("super+q")
        until("normal quit", lambda: not windows())
        w = launch()
        assert read_note() == "Draft\nKeep these exact bytes."
        machine.screenshot("05-reopened-draft")

    with subtest(
        "sticky native Close protects existing file then retry closes only sticky"
    ):
        chord("super+t")
        s = until(
            "sticky window",
            lambda: next((s for s in windows() if s["title"] == "Draft"), None),
        )
        focus(s)
        e = until("sticky editor", lambda: find(s, name="sticky-editor"))
        x, y, width, height = e["shown"]
        g = s["geometry"]
        click(g["x"] + x + width / 2, g["y"] + y + height / 2)
        chord("super+Down")
        user("chmod 500 ~/Notes/QA")
        user("chmod 400 ~/Notes/QA/Draft.md")
        text(" Sticky edit.")
        until("sticky save error", lambda: has_error(s))
        assert read_note() == "Draft\nKeep these exact bytes."
        user(f"aizomectl resize {s['id']} {g['x']} {g['y']} 160 100")
        s = until(
            "minimum sticky size",
            lambda: next(
                (
                    s
                    for s in windows()
                    if s["title"] == "Draft"
                    and s["geometry"]["w"] == 160
                    and s["geometry"]["h"] == 100
                ),
                None,
            ),
        )
        retry = until(
            "minimum sticky Retry", lambda: find(s, role="button", label="Retry")
        )
        assert retry["shown"][3] >= 20
        editor = find(s, name="sticky-editor")
        assert editor is not None and editor["shown"][3] >= 24
        machine.screenshot("06a-minimum-sticky-error")
        user("chmod 700 ~/Notes/QA")
        user("chmod 600 ~/Notes/QA/Draft.md")
        button(s, "Retry")
        until("minimum sticky saved", lambda: not has_error(s))
        assert read_note() == "Draft\nKeep these exact bytes. Sticky edit."
        machine.screenshot("06b-minimum-sticky-recovered")
        # Restore ordinary geometry, then make a second failed edit for Close.
        user(f"aizomectl resize {s['id']} {g['x']} {g['y']} 260 240")
        s = until(
            "ordinary sticky size",
            lambda: next(
                (
                    s
                    for s in windows()
                    if s["title"] == "Draft" and s["geometry"]["w"] == 260
                ),
                None,
            ),
        )
        g = s["geometry"]
        e = find(s, name="sticky-editor")
        x, y, width, height = e["shown"]
        click(g["x"] + x + width / 2, g["y"] + y + height / 2)
        chord("super+Down")
        user("chmod 500 ~/Notes/QA")
        user("chmod 400 ~/Notes/QA/Draft.md")
        text(" Close edit.")
        until("second sticky save error", lambda: has_error(s))
        # The ordinary decoration's close button sends the native close request.
        click(g["x"] + 10, g["y"] - 12)
        until(
            "sticky close protected", lambda: find(s, role="button", label="Keep Open")
        )
        machine.screenshot("06-sticky-close-protected")
        user("chmod 700 ~/Notes/QA")
        user("chmod 600 ~/Notes/QA/Draft.md")
        button(s, "Retry")
        until("sticky closed", lambda: len(windows()) == 1)
        assert windows()[0]["title"] == "Fusen"
        assert read_note() == "Draft\nKeep these exact bytes. Sticky edit. Close edit."
        machine.screenshot("07-sticky-recovered")

    with subtest("Quit retry saves before exiting"):
        w = windows()[0]
        focus(w)
        e = until("main editor", lambda: find(w, name="editor"))
        x, y, width, height = e["shown"]
        g = w["geometry"]
        click(g["x"] + x + width / 2, g["y"] + y + height / 2)
        chord("super+Down")
        user("chmod 500 ~/Notes/QA")
        user("chmod 400 ~/Notes/QA/Draft.md")
        text(" Quit edit.")
        until("main error", lambda: has_error(w))
        chord("super+q")
        until("Quit Retry", lambda: find(w, role="button", label="Keep Open"))
        machine.screenshot("08-quit-retry")
        user("chmod 700 ~/Notes/QA")
        user("chmod 600 ~/Notes/QA/Draft.md")
        button(w, "Retry")
        until("Quit exited after saving", lambda: not windows())
        assert (
            read_note()
            == "Draft\nKeep these exact bytes. Sticky edit. Close edit. Quit edit."
        )
        w = launch()
        machine.screenshot("09-final-reopen")
        assert not has_error(w)
        chord("super+q")
        until("final clean quit", lambda: not windows())
        machine.fail("coredumpctl --no-pager list aizome-fusen")
finally:
    machine.screenshot("final")
    Path(machine.out_dir, "inputs.json").write_text(json.dumps(inputs, indent=2))
    machine.succeed("journalctl _UID=1000 --no-pager > /tmp/fusen-save.log")
    machine.copy_from_machine("/tmp/fusen-save.log")
    machine.copy_from_machine(INSPECT)
    machine.shutdown()
