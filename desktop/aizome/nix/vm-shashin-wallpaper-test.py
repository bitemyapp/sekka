"""Component regression: real menus, saved paths and actual desktop pixels.

The fixture VM is independent of ISO qualification. Inspection only locates
painted controls and reads status; it never injects application messages.
"""

import json
import shlex
import time
from pathlib import Path

from PIL import Image

APP = "org.aizome.Shashin"
INSPECT = "/run/user/1000/wallpaper-inspect"
PICTURES = "/home/alice/Pictures"
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
        result = check()
        if result:
            return result
        time.sleep(0.2)
    machine.screenshot("failed")
    machine.succeed("cat " + SETTINGS + " > /tmp/failed-settings.toml 2>/dev/null || true")
    machine.succeed("journalctl _UID=1000 --no-pager > /tmp/failed-wallpaper.log")
    machine.copy_from_machine("/tmp/failed-settings.toml")
    machine.copy_from_machine("/tmp/failed-wallpaper.log")
    raise AssertionError(description)


def window():
    return next(
        (w for w in json.loads(user("aizomectl --json windows"))["windows"] if w["app_id"] == APP),
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
    return next((json.loads(d)["elements"] for d in docs if json.loads(d)["kind"] == "window"), [])


def status(prefix):
    return next((e["text"] for e in elements() if e["visible"] and e.get("text", "").startswith(prefix)), None)


def key(name):
    user("aizomectl input key " + name)
    time.sleep(0.15)


def open_picture(name):
    user(f"aizomectl spawn env AIZOME_KIT_INSPECT={INSPECT} aizome-shashin {PICTURES}/{name}")
    until("picture " + name, lambda: window() and window()["title"] == name)
    until("painted picture", lambda: any(e.get("name") == "stage" for e in elements()))
    time.sleep(0.3)


def wallpaper(which, screenshot=None, cancel=False):
    # A repeated error must be a fresh completion, not the previous
    # action's still-visible six-second status message.
    until("previous status expired", lambda: not status("Wallpaper unchanged:") and not status("Set as"))
    # The fixed 1280x800 test display's visible Edit title. End selects
    # its final item, Set as Wallpaper; Right opens the normal submenu.
    user("aizomectl input motion 224 12 && aizomectl input click")
    time.sleep(0.2)
    key("End")
    key("Right")
    key("Home")
    for _ in range(["Night", "Day", "Both"].index(which)):
        key("Down")
    if screenshot:
        machine.screenshot(screenshot)
    if cancel:
        key("Escape")
        key("Escape")
    else:
        key("Return")
        if screenshot:
            machine.screenshot(screenshot + "-result")


def settings():
    script = f"import tomllib,json; print(json.dumps(tomllib.load(open('{SETTINGS}','rb'))))"
    return json.loads(machine.succeed("python3 -c " + shlex.quote(script)))["appearance"]


def saved_bytes():
    return machine.succeed("sha256sum " + SETTINGS).split()[0]


def pixel(name):
    # Clear of the app (x >= 40), bar and Deskbar. This is the desktop
    # output, not a picture rendered by a separate test decoder.
    machine.screenshot(name)
    return Image.open(Path(machine.out_dir) / (name + ".png")).convert("RGB").getpixel((20, 400))


def assert_color(expected, name):
    # Wallpaper decoding/resizing is asynchronous; wait for the actual
    # desktop image, tolerating JPEG's small rounding differences.
    until("wallpaper pixels " + name, lambda: max(abs(a - b) for a, b in zip(pixel(name), expected)) <= 3)


machine.start()
machine.wait_until_succeeds("pgrep -u alice -x aizome-shell", timeout=120)
machine.succeed("install -d -o alice -g users " + PICTURES)
machine.succeed("install -o alice -g users /etc/wallpaper-pictures/* " + PICTURES)
user("mkdir -p " + INSPECT + " ~/.config/aizome")
initial = f'''[appearance]
theme = "night"
follow_sunset = false
wallpaper_night = "{PICTURES}/base.png"
wallpaper_day = "{PICTURES}/base.png"
'''
user("printf %s " + shlex.quote(initial) + " > " + SETTINGS)
user("aizomectl reload-config")
assert_color((192, 40, 40), "01-initial-wallpaper")
session = machine.succeed("pgrep -u alice -x aizome; pgrep -u alice -x aizome-shell")
open_picture("picture.svg")
machine.screenshot("02-svg-viewer")

with subtest("SVG refuses every destination and preserves desktop and settings"):
    before = saved_bytes()
    for which in ["Night", "Day", "Both"]:
        wallpaper(which, "03-svg-menu-" + which)
        message = until("honest SVG wallpaper error", lambda: status("Wallpaper unchanged:"))
        assert "SVG isn't supported" in message, message
        assert saved_bytes() == before
        assert_color((192, 40, 40), "04-svg-unchanged-" + which)
    machine.screenshot("05-svg-error")

with subtest("cancel does not replace either wallpaper"):
    open_picture("picture.png")
    before = saved_bytes()
    wallpaper("Both", "06-cancel-menu", cancel=True)
    assert saved_bytes() == before
    assert_color((192, 40, 40), "07-cancel-unchanged")

with subtest("all supported rasters and all destinations render and persist"):
    colors = {
        "png": (40, 80, 192), "jpg": (192, 80, 40), "webp": (80, 40, 192),
        "bmp": (40, 192, 160), "tiff": (192, 160, 40), "ico": (160, 40, 192),
        "gif": (40, 192, 80), "svg": (40, 80, 192),
    }
    for index, name in enumerate(["picture.png", "picture.jpg", "picture.webp", "picture.bmp", "picture.tiff", "picture.ico", "picture.gif", "raster.svg"]):
        open_picture(name)
        # Every consecutive case has a different color, so the previous
        # asynchronous wallpaper cannot satisfy the next assertion.
        expected = colors[name.rsplit(".", 1)[1]]
        # Each valid image follows a real selection. Day must preserve the
        # current Night path; Both must subsequently update both paths.
        previous_night = settings()["wallpaper_night"]
        wallpaper("Day")
        until("day path saved", lambda: settings()["wallpaper_day"] == PICTURES + "/" + name)
        assert settings()["wallpaper_night"] == previous_night
        wallpaper("Both")
        until("both paths saved", lambda: settings()["wallpaper_night"] == PICTURES + "/" + name)
        assert settings()["wallpaper_day"] == PICTURES + "/" + name
        assert_color(expected, f"08-raster-{index}")
        assert status("Set as")

with subtest("damaged or inaccessible paths cannot replace the existing wallpaper"):
    before = saved_bytes()
    for case in ["text", "truncated", "missing", "unreadable", "directory"]:
        name = "changed-" + case + ".png"
        path = PICTURES + "/" + name
        machine.succeed("install -o alice -g users /etc/wallpaper-pictures/base.png " + path)
        open_picture(name)
        if case == "text":
            machine.succeed("printf 'not an image' > " + path)
        elif case == "truncated":
            # Leave the PNG header and start of IDAT intact so the
            # renderer can read dimensions, but cannot decode pixels.
            script = f"from pathlib import Path; p=Path('{path}'); b=p.read_bytes(); p.write_bytes(b[:b.index(b'IDAT')+6])"
            machine.succeed("python3 -c " + shlex.quote(script))
        elif case == "missing":
            machine.succeed("rm " + path)
        elif case == "unreadable":
            machine.succeed("chmod 000 " + path)
        else:
            machine.succeed("rm " + path + "; mkdir " + path)
        wallpaper("Both")
        message = until("error for " + case, lambda: status("Wallpaper unchanged:"))
        if case == "truncated":
            assert "image data couldn't be decoded" in message, message
        assert saved_bytes() == before, case
        assert_color((40, 80, 192), "09-preserved-" + case)

with subtest("recovery and reopening retain the selected source path"):
    open_picture("base.png")
    wallpaper("Night")
    until("recovered Night path", lambda: settings()["wallpaper_night"] == PICTURES + "/base.png")
    assert settings()["wallpaper_day"] == PICTURES + "/raster.svg"
    assert_color((192, 40, 40), "10-recovered")
    user("aizomectl input chord super+q")
    until("app closed", lambda: window() is None)
    # Re-read the persisted configuration through the real desktop.
    user("aizomectl reload-config")
    assert_color((192, 40, 40), "11-persisted-desktop")
    open_picture("base.png")
    assert settings()["wallpaper_night"] == PICTURES + "/base.png"
    assert machine.succeed("pgrep -u alice -x aizome; pgrep -u alice -x aizome-shell") == session

machine.copy_from_machine(SETTINGS)
machine.succeed("journalctl _UID=1000 --no-pager > /tmp/shashin-wallpaper.log")
machine.copy_from_machine("/tmp/shashin-wallpaper.log")
machine.fail("coredumpctl --no-pager list aizome-shashin")
