"""Component regression: packaged Utsushi in a real Aizome session.

A video opened by MIME type plays to its end through PipeWire; a song
pauses and resumes by its key. Inspection only reads what the window
shows; input goes through the compositor. This fixture VM renders in
software, so it exercises the shared-memory frame path, not VA-API.
"""

import json
import re
import shlex
import time

APP = "org.aizome.Utsushi"
INSPECT = "/run/user/1000/utsushi-inspect"
MEDIA = "/etc/utsushi-media"


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


def until(description, check, timeout=30):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        result = check()
        if result:
            return result
        time.sleep(0.25)
    machine.screenshot("failed")
    machine.succeed("journalctl _UID=1000 --no-pager > /tmp/failed-utsushi.log")
    machine.copy_from_machine("/tmp/failed-utsushi.log")
    raise AssertionError(description)


def window(app=APP):
    return next(
        (w for w in json.loads(user("aizomectl --json windows"))["windows"] if w["app_id"] == app),
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


def timeline():
    """The timeline's value: the time shown, as m:ss."""
    return next((e.get("value") for e in elements() if e.get("name") == "timeline"), None)


def seconds(clock):
    m, s = clock.split(":")[-2:]
    return int(m) * 60 + int(s)


def stats():
    """The last stats line Utsushi logged (UTSUSHI_STATS, every 5 s from
    its start, ending with where playback was)."""
    lines = [
        l for l in machine.succeed("journalctl _UID=1000 --no-pager -o cat").splitlines() if l.startswith("utsushi: ")
    ]
    return lines[-1] if lines else ""


def stream():
    """Utsushi's PipeWire stream node, if it has one."""
    nodes = json.loads(user("pw-dump"))
    for n in nodes:
        props = (n.get("info") or {}).get("props") or {}
        if n.get("type") == "PipeWire:Interface:Node" and props.get("application.id") == APP:
            return props
    return None


def at_the_end(clip):
    """The last stats line, once it was logged at the clip's end (an
    earlier one counts frames shown before the end)."""
    m = re.search(r"shown: (\d+), dropped: (\d+).* at ([0-9.]+)$", stats())
    return m if m and float(m.group(3)) >= clip - 0.5 else None


def held(secs=2.0):
    """The time shown, if it stays put for `secs`."""
    before = timeline()
    time.sleep(secs)
    return before if before is not None and timeline() == before else None


machine.wait_for_unit("multi-user.target")
# The session is up once its shell is; aizomectl talks to the compositor's
# own socket, which comes a moment after Wayland's.
machine.wait_until_succeeds("pgrep -u alice -x aizome-shell", timeout=120)
machine.wait_until_succeeds("ls /run/user/1000 | grep -q '^aizome\\.wayland-[0-9]*\\.sock$'", timeout=120)
user(f"mkdir -p {INSPECT}")
# The first-run tour opens by itself: close it, so it can't take the
# keyboard from Utsushi.
tour = until("the first-run tour", lambda: window("org.aizome.Annai"), timeout=60)
user(f"aizomectl close {tour['id']}")
until("the tour closed", lambda: window("org.aizome.Annai") is None)

with subtest("the module lets streams ask for long cycles"):
    settings = user("pw-metadata -n settings 0 clock.max-quantum")
    assert "8192" in settings, settings

with subtest("a video opens as the desktop opens videos"):
    # The session's own default for video/mp4, through xdg-open; the
    # environment reaches the app through the first launch.
    user(
        f"aizomectl spawn env UTSUSHI_STATS=1 AIZOME_KIT_INSPECT={INSPECT} "
        f"xdg-open {MEDIA}/pattern.mp4"
    )
    until("Utsushi's window", lambda: window() and window()["title"] == "Test Pattern")
    props = until("its PipeWire stream", stream)
    assert props.get("media.role") == "Movie", props
    assert props.get("node.latency", "").startswith("2048/"), props

with subtest("it plays to the end"):
    until("the end", lambda: timeline() in ("0:05", "0:06"), timeout=40)
    line = until("its stats at the end", lambda: at_the_end(6.0), timeout=20)
    shown, dropped = int(line.group(1)), int(line.group(2))
    assert shown >= 100, line.group(0)
    assert "path: Software" in line.group(0), line.group(0)
    machine.screenshot("utsushi-video")

with subtest("a song pauses and goes on from where it stopped"):
    user(f"aizomectl spawn aizome-utsushi {MEDIA}/note.flac")
    until("the song", lambda: window() and window()["title"] == "Long Note")
    until("playing", lambda: timeline() not in (None, "0:00"))
    props = until("its stream", stream)
    assert props.get("media.role") == "Music", props
    assert props.get("node.latency", "").startswith("8192/"), props
    user(f"aizomectl focus {window()['id']}")
    user("aizomectl input key space")
    paused = until("paused", held, timeout=15)
    user("aizomectl input key space")
    until("playing again", lambda: seconds(timeline()) > seconds(paused), timeout=10)
    machine.screenshot("utsushi-song")
