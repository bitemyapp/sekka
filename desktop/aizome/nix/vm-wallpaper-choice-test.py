"""Real Settings input; inspect painted locations and actual desktop pixels."""

import json
import shlex
import time
from pathlib import Path

from PIL import Image

APP = "org.aizome.Settei"
INSPECT = "/run/user/1000/wallpaper-choice-inspect"
SETTINGS = "/home/alice/.config/aizome/settings.toml"


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


def until(description, check):
    deadline = time.monotonic() + 20
    while time.monotonic() < deadline:
        value = check()
        if value:
            return value
        time.sleep(0.2)
    machine.screenshot("failed")
    machine.succeed(
        "cat " + SETTINGS + " > /tmp/failed-settings.toml 2>/dev/null || true"
    )
    machine.copy_from_machine("/tmp/failed-settings.toml")
    raise AssertionError(description)


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
    w = window()
    if w is None:
        return []
    docs = user(
        f"for file in {INSPECT}/{APP}.{w['pid']}.*.json; do "
        'if test -f "$file"; then cat "$file"; printf "\\n"; fi; done'
    ).splitlines()
    return next(
        (json.loads(d)["elements"] for d in docs if json.loads(d)["kind"] == "window"),
        [],
    )


def find(**attrs):
    return next(
        (
            e
            for e in elements()
            if e["visible"] and all(e.get(k) == v for k, v in attrs.items())
        ),
        None,
    )


def click_at(x, y):
    g = window()["geometry"]
    user(f"aizomectl input motion {g['x'] + x} {g['y'] + y}")
    user("aizomectl input click")
    time.sleep(0.3)


def click(**attrs):
    e = until(str(attrs), lambda: find(**attrs))
    x, y, width, height = e["shown"]
    click_at(x + width / 2, y + height / 2)


def pane(name):
    user(
        f"aizomectl spawn env AIZOME_KIT_INSPECT={INSPECT} aizome-settei --pane appearance.{name}"
    )
    until("settings pane " + name, lambda: find(name="entry:appearance." + name))
    time.sleep(0.3)
    if name == "wallpaper":
        # Revealing the named row does not also reveal its picture grid.
        # Scroll the ordinary pane until the actual choice is clickable.
        for _ in range(6):
            tile = find(label="No picture", role="button")
            if tile is not None and tile["shown"][3] >= 48:
                break
            g = window()["geometry"]
            user(
                f"aizomectl input motion {g['x'] + g['w'] - 100} {g['y'] + g['h'] - 120}"
            )
            user("aizomectl input scroll 0 160")
            time.sleep(0.2)


def theme(name):
    pane("theme")
    click(label=name, role="button")


def wallpaper_tab(day):
    pane("wallpaper")
    row = until("wallpaper row", lambda: find(name="entry:appearance.wallpaper"))
    x, y, width, height = row["rect"]
    click_at(x + width - (25 if day else 75), y + height / 2)
    until(
        "wallpaper tab",
        lambda: find(text="Wallpaper for " + ("Day" if day else "Night")),
    )


def settings():
    script = (
        "import json,tomllib; print(json.dumps(tomllib.load(open("
        + repr(SETTINGS)
        + ",'rb'))))"
    )
    return json.loads(machine.succeed("python3 -c " + shlex.quote(script)))[
        "appearance"
    ]


def color(expected, name):
    def matches():
        machine.screenshot(name)
        pixel = (
            Image.open(Path(machine.out_dir) / (name + ".png"))
            .convert("RGB")
            .getpixel((20, 400))
        )
        assert isinstance(pixel, tuple)
        return max(abs(a - b) for a, b in zip(pixel, expected)) <= 3

    until("desktop pixels " + name, matches)


machine.start()
machine.wait_until_succeeds("pgrep -u alice -x aizome-shell", timeout=120)
user("mkdir -p " + INSPECT)
identity = machine.succeed("pgrep -u alice -x aizome; pgrep -u alice -x aizome-shell")
color((192, 40, 40), "01-system-night")

with subtest("No picture overrides a supplied Night default"):
    pane("wallpaper")
    click(label="No picture", role="button")
    color((22, 22, 30), "02-plain-night")
    assert settings()["wallpaper_night"] == ""
    assert "wallpaper_day" not in settings()

with subtest("Night and Day choices are independent"):
    theme("Day")
    color((40, 192, 80), "03-system-day")
    wallpaper_tab(True)
    click(label="No picture", role="button")
    color((201, 205, 224), "04-plain-day")
    assert settings()["wallpaper_day"] == settings()["wallpaper_night"] == ""
    theme("Night")
    color((22, 22, 30), "05-night-still-plain")

with subtest("Reopening Settings retains an explicit blank choice"):
    before = machine.succeed("sha256sum " + SETTINGS)
    user("aizomectl input chord super+q")
    until("settings quit", lambda: window() is None)
    pane("wallpaper")
    assert before == machine.succeed("sha256sum " + SETTINGS)
    color((22, 22, 30), "06-reopened-blank")

with subtest("System default restores only the chosen theme"):
    click(label="System default", role="button")
    color((192, 40, 40), "07-restored-night")
    assert "wallpaper_night" not in settings()
    assert settings()["wallpaper_day"] == ""
    theme("Day")
    color((201, 205, 224), "08-day-still-plain")
    wallpaper_tab(True)
    click(label="System default", role="button")
    color((40, 192, 80), "09-restored-day")
    assert "wallpaper_night" not in settings() and "wallpaper_day" not in settings()
    theme("Night")
    color((192, 40, 40), "10-default-night-again")

user("aizomectl input chord super+q")
until("normal quit", lambda: window() is None)
assert (
    machine.succeed("pgrep -u alice -x aizome; pgrep -u alice -x aizome-shell")
    == identity
)
machine.succeed("journalctl _UID=1000 --no-pager > /tmp/wallpaper-choice.log")
machine.copy_from_machine("/tmp/wallpaper-choice.log")
machine.fail("coredumpctl --no-pager list aizome-settei")
