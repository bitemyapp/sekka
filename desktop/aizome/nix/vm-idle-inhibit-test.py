"""Component regression: a playing video keeps an Aizome session awake.

The settings lock the screen and turn the displays off after a minute.
With a looping video playing in Utsushi, in mpv and in VLC, each started
as its desktop entry starts it, the session does neither for well over
the minute. Once each is paused it does both, and so it does when VLC is
killed while it plays.

Utsushi and mpv hold a Wayland idle inhibitor. VLC, an X11 client here,
asks org.freedesktop.ScreenSaver on the session bus, which the shell
serves: its log says who holds, who let go and who left holding. The
desktop portal's Inhibit, which Firefox ends at when nobody serves the
screen saver, is asked once to see that it reaches the same place.

Nothing touches the keyboard or pointer while the session is watched:
state is read over the compositor's IPC, which isn't input.
"""

import json
import shlex
import time

MEDIA = "/etc/idle-inhibit-media/pattern.mp4"
ENV = (
    "export XDG_RUNTIME_DIR=/run/user/1000; "
    "export DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus; "
    "export WAYLAND_DISPLAY=$(ls /run/user/1000 | grep -m1 '^wayland-[0-9]*$'); "
    "export AIZOME_SOCKET=/run/user/1000/aizome.$WAYLAND_DISPLAY.sock; "
)
# The settings' minute, the compositor's five seconds between looks at
# the clock, and room for a busy host.
IDLE = 60
AWAKE_FOR = IDLE + 20
IDLE_WITHIN = IDLE + 40
MPRIS = "/org/mpris/MediaPlayer2 org.mpris.MediaPlayer2.Player"
# What the shell's screen saver logs of an app's hold.
HELD = "keeps the screen awake"
LET_GO = "lets the screen sleep"
LEFT = "left the bus holding the screen awake"


def user(command):
    return machine.succeed("su - alice -c " + shlex.quote(ENV + command))


def ipc(request):
    return json.loads(user("aizomectl --json " + request))


def windows():
    return ipc("windows")["windows"]


def window(app):
    return next((w for w in windows() if w["app_id"] == app), None)


def locked():
    return ipc("recovery status")["state"]["locked"]


def displays_on():
    return all(o["powered"] for o in ipc("outputs")["outputs"])


def said(app, what):
    """How many times the shell's screen saver has said `what` of the app
    that calls itself `app`."""
    lines = machine.succeed("journalctl _UID=1000 --no-pager -o cat").splitlines()
    return sum(1 for line in lines if f'"{app}" (:' in line and what in line)


def fail(description):
    machine.screenshot("failed")
    machine.succeed("journalctl -b --no-pager > /tmp/failed-idle-inhibit.log")
    machine.copy_from_machine("/tmp/failed-idle-inhibit.log")
    raise AssertionError(description)


def within(check, timeout):
    """What `check` gives once it gives something, or None after `timeout`."""
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        result = check()
        if result:
            return result
        time.sleep(0.5)
    return None


def until(description, check, timeout=30):
    return within(check, timeout) or fail(description)


def stays_awake(what):
    """Nobody touches anything for longer than the idle minute: the
    session neither locks nor turns the displays off."""
    start = time.monotonic()
    while (waited := time.monotonic() - start) < AWAKE_FOR:
        if locked() or not displays_on():
            fail(f"{what}: the session went idle after {waited:.0f} s (locked: {locked()}, displays on: {displays_on()})")
        time.sleep(5)
    print(f"{what}: awake after {AWAKE_FOR} s without input")


def goes_idle(what):
    """Nobody touches anything: the session locks and the displays turn off."""
    start = time.monotonic()
    until(f"{what}: the session never locked", locked, timeout=IDLE_WITHIN)
    until(f"{what}: the displays stayed on", lambda: not displays_on(), timeout=10)
    print(f"{what}: locked and dark after {time.monotonic() - start:.0f} s without input")


def wake():
    """A key lights the displays; the password unlocks."""
    machine.send_key("shift")
    until("the displays to light", displays_on)
    # The lock screen's field may still be coming back with the displays:
    # a password typed too soon is refused, and typed again.
    for _ in range(3):
        machine.send_chars("aizome\n")
        if within(lambda: not locked(), 10):
            return
    fail("the password didn't unlock the screen")


def mpris(player, verb, what):
    """busctl's `verb` (call, get-property, set-property) on a player's MPRIS."""
    return user(f"busctl --user {verb} org.mpris.MediaPlayer2.{player} {MPRIS} {what}")


def playing(player):
    """Its position moves (MPRIS, in microseconds)."""

    def position():
        return int(mpris(player, "get-property", "Position").split()[1])

    before = position()
    time.sleep(1)
    return position() != before


def desktop_exec(entry):
    return machine.succeed(f"grep -m1 '^Exec=' /run/current-system/sw/share/applications/{entry}.desktop").strip()


def close(app):
    for w in windows():
        if w["app_id"] == app:
            user(f"aizomectl close {w['id']}")
    until(f"{app}'s windows to close", lambda: window(app) is None)


machine.wait_for_unit("multi-user.target")
machine.wait_until_succeeds("pgrep -u alice -x aizome-shell", timeout=120)
machine.wait_until_succeeds("ls /run/user/1000 | grep -q '^aizome\\.wayland-[0-9]*\\.sock$'", timeout=120)
# The first-run tour opens by itself: close it, so it can't take the
# keyboard from a player.
tour = until("the first-run tour", lambda: window("org.aizome.Annai"), timeout=60)
user(f"aizomectl close {tour['id']}")
until("the tour closed", lambda: window("org.aizome.Annai") is None)

with subtest("the settings ask for a minute, and nothing but the session's own idleness puts it to sleep"):
    settings = user("cat ~/.config/aizome/settings.toml")
    assert "lock_after_minutes = 1" in settings and "monitors_off_after_minutes = 1" in settings, settings
    # logind does nothing of its own when the session is idle.
    action = machine.succeed(
        "busctl get-property org.freedesktop.login1 /org/freedesktop/login1 org.freedesktop.login1.Manager IdleAction"
    )
    assert action.strip() == 's "ignore"', action

with subtest("Utsushi playing holds idleness off, and paused lets it come"):
    # As the desktop opens a video: the session's default for video/mp4.
    user(f"aizomectl spawn xdg-open {MEDIA}")
    until("Utsushi's window", lambda: (w := window("org.aizome.Utsushi")) and w["title"] == "Test Pattern")
    until("Utsushi on the bus", lambda: "utsushi" in user("busctl --user list --no-pager"))
    mpris("utsushi", "set-property", "LoopStatus s Track")
    until("Utsushi playing", lambda: playing("utsushi"))
    stays_awake("Utsushi playing")
    assert playing("utsushi"), "Utsushi stopped playing"
    machine.screenshot("utsushi-playing")
    mpris("utsushi", "call", "Pause")
    until("Utsushi paused", lambda: not playing("utsushi"))
    goes_idle("Utsushi paused")
    wake()
    close("org.aizome.Utsushi")

with subtest("mpv playing holds idleness off, and paused lets it come"):
    assert desktop_exec("mpv") == "Exec=mpv --player-operation-mode=pseudo-gui -- %U", desktop_exec("mpv")
    user(f"aizomectl spawn mpv --player-operation-mode=pseudo-gui --loop-file=inf -- {MEDIA}")
    mpv = until("mpv's window", lambda: window("mpv"), timeout=60)
    # A Wayland client of its own, not one of Xwayland's.
    assert machine.succeed(f"cat /proc/{mpv['pid']}/comm").strip() == "mpv", mpv
    stays_awake("mpv playing")
    machine.screenshot("mpv-playing")
    user(f"aizomectl focus {mpv['id']}")
    user("aizomectl input key space")
    goes_idle("mpv paused")
    wake()
    close("mpv")

with subtest("VLC playing holds idleness off; paused, and killed, it lets it come"):
    assert desktop_exec("vlc").endswith("/bin/vlc --started-from-file %U"), desktop_exec("vlc")
    user(f"aizomectl spawn vlc --started-from-file --loop {MEDIA}")
    # Its first run asks about network access before anything else: Continue.
    until("VLC's first-run question", lambda: any("Privacy" in w["title"] for w in windows()), timeout=90)
    user("aizomectl input key return")
    until("the question answered", lambda: not any("Privacy" in w["title"] for w in windows()))
    vlc = until("VLC's window", lambda: window("vlc"))
    # An X11 client: its window is xwayland-satellite's.
    assert "xwayland-satel" in machine.succeed(f"cat /proc/{vlc['pid']}/comm"), vlc
    until("VLC on the bus", lambda: "org.mpris.MediaPlayer2.vlc" in user("busctl --user list --no-pager"))
    until("VLC playing", lambda: playing("vlc"), timeout=60)
    stays_awake("VLC playing")
    assert playing("vlc"), "VLC stopped playing"
    assert said("vlc", HELD) > 0, "VLC played on without asking the shell's screen saver"
    machine.screenshot("vlc-playing")

    let_go = said("vlc", LET_GO)
    mpris("vlc", "call", "Pause")
    until("VLC paused", lambda: not playing("vlc"))
    until("VLC gave its cookie back", lambda: said("vlc", LET_GO) > let_go)
    goes_idle("VLC paused")
    wake()

    # Playing again, it holds again; killed, it can't say it lets go.
    held, left = said("vlc", HELD), said("vlc", LEFT)
    mpris("vlc", "call", "Play")
    until("VLC playing again", lambda: playing("vlc"))
    until("VLC's hold again", lambda: said("vlc", HELD) > held)
    machine.succeed("pkill -KILL -u alice -x .vlc-wrapped")
    until("VLC gone", lambda: window("vlc") is None)
    until("its hold gone with it", lambda: said("vlc", LEFT) > left)
    goes_idle("VLC killed while playing")
    wake()

with subtest("an app that asks the desktop portal reaches the same screen saver"):
    # org.freedesktop.portal.Inhibit, for idleness (8): Aizome's sessions
    # answer it with the GTK portal, which asks the session bus's screen
    # saver in turn, with no name for an app that isn't sandboxed. This
    # caller leaves at once, and the portal lets go for it.
    user(
        "busctl --user call org.freedesktop.portal.Desktop /org/freedesktop/portal/desktop "
        "org.freedesktop.portal.Inhibit Inhibit 'sua{sv}' '' 8 1 reason s 'a film'"
    )
    until("the portal's hold", lambda: said("", HELD + ': "a film"') > 0)
    until("the portal let go", lambda: said("", LET_GO) > 0)
