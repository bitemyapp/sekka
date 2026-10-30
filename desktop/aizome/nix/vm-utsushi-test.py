"""Component regression: packaged Utsushi in a real Aizome session.

A video opened by MIME type plays to its end through PipeWire; a song
pauses and resumes by its key; a sound the shell plays over the song
gets short graph cycles, and the song its long ones back after it.
Inspection only reads what the window shows; input goes through the
compositor. This fixture VM renders in software, so it exercises the
shared-memory frame path, not VA-API.
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


def player_stream():
    """The stream node of a `pw-play` running, if there is one, with the
    process of its client as `pid`."""
    objects = json.loads(user("pw-dump"))
    for n in objects:
        props = (n.get("info") or {}).get("props") or {}
        names = {props.get(k) for k in ("application.process.binary", "application.name", "node.name")}
        if n.get("type") == "PipeWire:Interface:Node" and names & {"pw-play", "pw-cat"}:
            client = next((o for o in objects if o.get("id") == props.get("client.id")), {})
            of_client = (client.get("info") or {}).get("props") or {}
            return dict(props, pid=of_client.get("application.process.id") or of_client.get("pipewire.sec.pid"))
    return None


def graph_cycle():
    """The cycle the graph runs at, in frames: pw-top's QUANT for the sink,
    which drives it, in its last report."""
    report = re.split(r"^S\s+ID\s+QUANT\b.*$", user("timeout 30 pw-top -b -n 3"), flags=re.M)[-1]
    print(report)
    for line in report.splitlines():
        fields = line.split()
        if len(fields) >= 4 and fields[-1] == "test-sink" and fields[2].isdigit():
            return int(fields[2])
    return None


def short_cycle():
    """The graph's cycle, if it's short (PipeWire's usual 1024 or less)."""
    cycle = graph_cycle()
    return cycle if cycle is not None and 0 < cycle <= 1024 else None


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

with subtest("a sound from the shell runs on short cycles over music's long ones"):
    # The song, on as it was: the graph at music's cycle.
    until("music's long cycles", lambda: graph_cycle() == 8192, timeout=40)
    # The shell plays the sound a notification asks for with pw-play.
    user(
        "DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus "
        f"notify-send --app-name=Chimes -h string:sound-file:{MEDIA}/chime.wav 'A chime'"
    )
    sound = until("the shell's sound", player_stream, timeout=10)
    print(f"the shell's sound: {sound}")
    pid = sound["pid"]
    assert pid, f"no process for the sound's client: {sound}"
    parent = user(f"ps -o comm= -p $(ps -o ppid= -p {pid})").strip()
    assert parent == "aizome-shell", f"pw-play {pid} came from {parent!r}"
    # 20 ms, in frames at the file's rate.
    frames, rate = (int(x) for x in sound.get("node.latency", "0/1").split("/"))
    assert abs(frames / rate - 0.020) < 0.001, sound
    cycle = until("a short cycle while it plays", short_cycle, timeout=8)
    print(f"with the sound and the song, the graph ran at {cycle} frames")
    # Once it's done, the song has the graph to itself again.
    until("the sound ended", lambda: player_stream() is None, timeout=20)
    until("music's long cycles again", lambda: graph_cycle() == 8192, timeout=20)
    assert window() and window()["title"] == "Long Note", "the song played on"
