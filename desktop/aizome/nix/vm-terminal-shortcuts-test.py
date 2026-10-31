"""Actual client input; read-only IPC/inspection verifies effects, not app messages."""

import json
import shlex
import time


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


def windows():
    return json.loads(user("aizomectl --json windows"))["windows"]


def feet():
    return [w for w in windows() if w["app_id"] == "foot"]


def hotarus():
    return [w for w in windows() if w["app_id"] == "org.aizome.Hotaru"]


def until(description, check):
    deadline = time.monotonic() + 20
    while time.monotonic() < deadline:
        result = check()
        if result:
            return result
        time.sleep(0.25)
    machine.screenshot("failed")
    raise AssertionError(description)


def chord(keys):
    user("aizomectl input chord " + keys)
    time.sleep(0.3)


def type_text(text):
    user("aizomectl input type " + shlex.quote(text))
    time.sleep(0.2)


def click(x, y):
    user(f"aizomectl input motion {x} {y}")
    user("aizomectl input click")
    time.sleep(0.3)


def focus(w):
    g = w["geometry"]
    click(g["x"] + g["w"] / 2, g["y"] + g["h"] / 2)


machine.start()
machine.wait_until_succeeds("pgrep -u alice -x aizome-shell", timeout=120)

with subtest("foot, chosen before Hotaru, becomes Hotaru once"):
    until(
        "the move to Hotaru",
        lambda: machine.execute("test -e /home/alice/.local/state/aizome/terminal-moved-to-hotaru")[0] == 0,
    )
    listed = machine.succeed("cat /home/alice/.config/xdg-terminals.list").split()
    assert listed == ["org.aizome.Hotaru.desktop", "foot.desktop"], listed

with subtest("❖Return opens Hotaru, the default terminal, and typing reaches its shell"):
    chord("super+Return")
    until("Hotaru", hotarus)

    def typed():
        # Retried: the shell may not have started when the window appears.
        type_text("printf typed-into-hotaru > /tmp/hotaru-typed")
        user("aizomectl input key Return")
        status, _ = machine.execute('test "$(cat /tmp/hotaru-typed)" = typed-into-hotaru')
        return status == 0

    until("a command typed into Hotaru runs", typed)
    machine.screenshot("00-hotaru-default")
    chord("super+q")
    until("Quit closes Hotaru", lambda: not hotarus())

# foot, a terminal outside Aizome's kit, gets its ❖ shortcuts translated.
with subtest("Close affects one terminal; Quit affects that terminal app"):
    user("aizomectl spawn foot")
    first = until("first terminal", lambda: feet() and feet()[0])
    user("aizomectl spawn foot")
    until("two terminals", lambda: len(feet()) == 2)
    machine.screenshot("01-two-terminals")
    chord("super+w")
    until("Close removes only current terminal", lambda: len(feet()) == 1)
    assert feet()[0]["id"] == first["id"]
    machine.screenshot("02-one-terminal")
    user("aizomectl spawn foot")
    until("two again", lambda: len(feet()) == 2)
    chord("super+q")
    until("Quit closes both terminals", lambda: not feet())
    machine.screenshot("03-terminal-quit")

with subtest("terminal paste and copy still use Ctrl+Shift"):
    user("aizomectl spawn foot")
    current = until("clipboard terminal", lambda: feet() and feet()[0])
    # Set a test clipboard, then exercise the real terminal's paste shortcut.
    user("printf 'clipboard-roundtrip' | wl-copy")
    type_text("printf '%s' '")
    chord("super+v")
    type_text("' > /tmp/terminal-pasted")
    user("aizomectl input key Return")
    machine.wait_until_succeeds(
        'test "$(cat /tmp/terminal-pasted)" = clipboard-roundtrip'
    )
    type_text("printf 'COPYTOKEN\\n'")
    user("aizomectl input key Return")
    # Search is another translated terminal command; its match is selected.
    chord("super+f")
    type_text("COPYTOKEN")
    machine.screenshot("04-terminal-search")
    user("aizomectl input key Return")
    chord("super+c")
    assert user("wl-paste --no-newline") == "COPYTOKEN"
    machine.screenshot("05-terminal-copy")
    chord("super+q")
    until("clipboard terminal closed", lambda: not feet())

with subtest("GTK keeps application control shortcuts"):
    user("aizomectl spawn python3 /etc/shortcut-probe.py")
    gtk = until(
        "GTK probe",
        lambda: next(
            (w for w in windows() if w["title"] == "GTK command translation"), None
        ),
    )
    focus(gtk)
    chord("super+w")
    chord("super+q")
    keys = machine.succeed("cat /tmp/gtk-keys")
    assert "w ctrl=True shift=False super=False" in keys, keys
    assert "q ctrl=True shift=False super=False" in keys, keys
    assert any(w["id"] == gtk["id"] for w in windows())
    machine.screenshot("06-gtk-keeps-commands")

with subtest("native document still owns its unsaved Close"):
    user("mkdir -p /run/user/1000/terminal-inspect")
    user(
        "aizomectl spawn env AIZOME_KIT_INSPECT=/run/user/1000/terminal-inspect aizome-sumi"
    )
    native = until(
        "Sumi",
        lambda: next((w for w in windows() if w["app_id"] == "org.aizome.Sumi"), None),
    )
    focus(native)
    type_text("unsaved terminal regression")
    chord("super+w")

    def alert():
        docs = user(
            "cat /run/user/1000/terminal-inspect/org.aizome.Sumi.*.json"
        ).splitlines()
        return any(
            e["visible"] and e.get("text") == "Cancel"
            for d in docs
            for e in json.loads(d)["elements"]
        )

    until("native unsaved confirmation", alert)
    machine.screenshot("07-native-unsaved")
    user("aizomectl input key Escape")
    until("native Cancel", lambda: not alert())
    assert any(w["id"] == native["id"] for w in windows())
    machine.screenshot("08-native-cancelled")

machine.succeed("journalctl _UID=1000 --no-pager > /tmp/terminal-shortcuts.log")
machine.copy_from_machine("/tmp/terminal-shortcuts.log")
machine.copy_from_machine("/tmp/gtk-keys")
machine.fail("coredumpctl --no-pager list foot")
machine.fail("coredumpctl --no-pager list | grep -i hotaru")
