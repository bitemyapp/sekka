"""Component regression: a playing video keeps an Aizome session awake.

The settings lock the screen and turn the displays off after a minute.
With a looping video playing in Utsushi, in mpv and in VLC, each started
as its desktop entry starts it, the session does neither for well over
the minute. Once each is paused it does both, and so it does when VLC is
killed while it plays. A page's video in Chromium does the same: in a
window and full screen, when it starts by itself with the person in
another window, and with Chromium as an X11 client.

Utsushi, mpv and Chromium hold a Wayland idle inhibitor. VLC, an X11
client here, asks org.freedesktop.ScreenSaver on the session bus, which
the shell serves: its log says who holds, who let go and who left
holding. Chromium asks it as well, and has nothing else as an X11
client, or for a video that starts while its window has neither the
keyboard nor the pointer. The desktop portal's Inhibit, which Firefox
ends at when nobody serves the screen saver, is asked once to see that it
reaches the same place.

While each plays, the cup at the front of the systray shows for it, and
its panel lists the app by its desktop entry's name with its reason (the
shell says what its bar and panel show): one row for Chromium, which
holds both ways, and none for the shell's own inhibitor. The cup goes
once the player has paused.

Those holds are for someone at the screen. With mpv playing, and with
VLC, the session is locked by hand (the lock's keys): the displays turn
off on their minute behind the lock though the video plays on, and
unlocked, it holds them on again. Keep Awake is the user's own switch:
turned on beside VLC, its cup takes the place of the apps', the panel
lists VLC under it, and locked by hand the displays stay on for as long
as it's on.

Nothing touches the keyboard or pointer while the session is watched:
state is read over the compositor's IPC and the shell's socket, which
aren't input.
"""

import json
import re
import shlex
import time

MEDIA = "/etc/idle-inhibit-media/pattern.mp4"
PAGE = "file:///etc/idle-inhibit-media/video.html"
# Chromium past its first run's questions, and without the keyring's.
CHROMIUM = "chromium --no-first-run --no-default-browser-check --password-store=basic"
# What Chromium calls itself to the screen saver: its program's path.
CHROMIUM_NAME = '[^"]*/chromium'
ENV = (
    "export XDG_RUNTIME_DIR=/run/user/1000; "
    "export DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus; "
    "export WAYLAND_DISPLAY=$(ls /run/user/1000 | grep -m1 '^wayland-[0-9]*$'); "
    "export AIZOME_SOCKET=/run/user/1000/aizome.$WAYLAND_DISPLAY.sock; "
    "export AIZOME_SHELL_SOCKET=/run/user/1000/aizome-shell.$WAYLAND_DISPLAY.sock; "
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
    return next((w for w in windows() if w["app_id"].lower() == app.lower()), None)


def locked():
    return ipc("recovery status")["state"]["locked"]


def displays_on():
    return all(o["powered"] for o in ipc("outputs")["outputs"])


def said(app, what):
    """How many times the shell's screen saver has said `what` of the app
    that calls itself `app` (a pattern)."""
    lines = machine.succeed("journalctl _UID=1000 --no-pager -o cat").splitlines()
    return sum(1 for line in lines if re.search(f'"{app}" \\(:', line) and what in line)


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


def awake():
    """What the shell says of Keep Awake, the cup in its bar, and the apps
    its panel lists as holding the screen on."""
    return json.loads(user("aizome-shell msg state"))["awake"]


def inhibitors():
    """Who the compositor says holds idleness off."""
    return ipc("idle-inhibitors")["inhibitors"]


def cup_lists(rows, shot):
    """The cup is in the bar for apps (Keep Awake is off), and its panel
    lists just these: each app's name, and what's said under it. A
    picture of the panel is kept."""

    def listed():
        state = awake()
        return state["cup"] == "apps" and [(h["name"], h["detail"]) for h in state["holders"]] == rows

    until(f"the cup, listing {rows}", listed, timeout=20)
    panel_shot(shot)
    print(f"the cup lists {rows}")


def panel_shot(shot):
    """A picture of the screen with the cup's panel open."""
    user("aizome-shell msg " + shlex.quote(json.dumps({"request": "open", "what": "keep-awake"})))
    until("the cup's panel", lambda: json.loads(user("aizome-shell msg state"))["panel"] == "keep-awake")
    time.sleep(1)
    machine.screenshot(shot)
    user("aizome-shell msg dismiss")
    until("the panel closed", lambda: json.loads(user("aizome-shell msg state"))["panel"] is None)


def cup_goes(what):
    """Nothing holds any more: the cup leaves the bar, a moment later."""
    start = time.monotonic()
    until(f"{what}: the cup stayed in the bar", lambda: awake()["cup"] is None and not awake()["holders"], timeout=20)
    print(f"{what}: the cup went {time.monotonic() - start:.1f} s later")


def lock_by_hand(what):
    """The lock's keys, as a person presses them."""
    assert displays_on() and not locked(), f"{what}: not awake to begin with"
    user("aizomectl input chord ctrl+super+q")
    until(f"{what}: the lock by hand", locked)


def goes_dark_locked(what, still_playing):
    """Locked, with nobody touching anything: the displays turn off on
    their minute though the video plays on."""
    start = time.monotonic()
    until(f"{what}: the displays stayed on behind the lock", lambda: not displays_on(), timeout=IDLE_WITHIN)
    waited = time.monotonic() - start
    assert locked() and still_playing(), f"{what}: it stopped playing behind the lock"
    assert waited > IDLE - 15, f"{what}: the displays went off after {waited:.0f} s, before their minute"
    print(f"{what}: locked, the displays went off {waited:.0f} s later, and it was still playing")


def stays_lit_locked(what):
    """Locked, with nobody touching anything for longer than the idle
    minute: the displays stay on."""
    start = time.monotonic()
    while (waited := time.monotonic() - start) < AWAKE_FOR:
        if not locked() or not displays_on():
            fail(f"{what}: after {waited:.0f} s locked: {locked()}, displays on: {displays_on()}")
        time.sleep(5)
    print(f"{what}: locked, the displays were still on after {AWAKE_FOR} s without input")


def holds_again_unlocked(what, still_playing):
    """A key and the password bring the session back, and with someone
    watching again the video holds the displays on: well past a few looks
    at the idle clock, nothing has locked or gone dark."""
    wake()
    time.sleep(12)
    assert displays_on() and not locked() and still_playing(), f"{what}: it didn't hold again once unlocked"


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
        if w["app_id"].lower() == app.lower():
            user(f"aizomectl close {w['id']}")
    until(f"{app}'s windows to close", lambda: window(app) is None)


def page():
    """What the page says of its video in Chromium's title: "playing 123
    windowed", "paused 45 fullscreen"."""
    return until("Chromium's window", lambda: window("chromium-browser"), timeout=120)["title"]


def page_playing():
    """It says playing, and its clock moves."""
    before = page()
    time.sleep(1)
    after = page()
    return before.startswith("playing") and after.startswith("playing") and before != after


def press(key):
    """A key to the page, as from the keyboard: p plays and pauses, f goes
    full screen and back."""
    chromium = until("Chromium's window", lambda: window("chromium-browser"))
    user(f"aizomectl focus {chromium['id']} && sleep 0.5 && aizomectl input key {key}")


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
    # Nothing plays yet: nobody holds the screen on, and there's no cup.
    assert inhibitors() == [], inhibitors()
    assert awake() == {"keep_awake": "off", "cup": None, "holders": []}, awake()

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
    # Its Wayland inhibitor gives no reason: the row says which window.
    cup_lists([("Utsushi", "Test Pattern")], "cup-utsushi")
    mpris("utsushi", "call", "Pause")
    until("Utsushi paused", lambda: not playing("utsushi"))
    cup_goes("Utsushi paused")
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
    mpv_holds = [(i["kind"], i.get("app_id"), i.get("window"), i.get("pid")) for i in inhibitors()]
    assert mpv_holds == [("app", "mpv", mpv["id"], mpv["pid"])], mpv_holds
    cup_lists([("mpv Media Player", window("mpv")["title"])], "cup-mpv")
    # Its hold is for someone at the screen: locked by hand, the displays
    # turn off on their minute. (mpv holds its inhibitor only while it
    # plays: still there, it plays.)
    lock_by_hand("mpv playing")
    goes_dark_locked("mpv playing", lambda: any(i.get("app_id") == "mpv" for i in inhibitors()))
    holds_again_unlocked("mpv playing", lambda: any(i.get("app_id") == "mpv" for i in inhibitors()))
    user(f"aizomectl focus {mpv['id']}")
    user("aizomectl input key space")
    cup_goes("mpv paused")
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
    # At the compositor the shell's own inhibitor stands for VLC's hold,
    # on a surface that's no window's. It isn't a row: VLC is, by its
    # desktop entry's name, with the reason it gave the bus.
    shell_pid = int(machine.succeed("pgrep -u alice -x aizome-shell").split()[0])
    # (Looked for more than once: VLC lets go for a moment at each loop
    # of the clip, and the shell's inhibitor with it.)
    until(
        "the shell's inhibitor alone at the compositor",
        lambda: [(i["kind"], i.get("window"), i.get("pid")) for i in inhibitors()] == [("app", None, shell_pid)],
    )
    cup_lists([("VLC media player", "Playing some media.")], "cup-vlc")

    # Keep Awake on as well. It's the user's own switch: its service asks
    # the compositor by a request of its own, the cup is Keep Awake's, and
    # the panel lists VLC under its switch still.
    print(user("aizome-awake on"))
    until("Keep Awake's cup", lambda: awake()["cup"] == "keep-awake")
    service = int(user("systemctl --user show -p MainPID --value aizome-keep-awake.service"))
    until(
        "Keep Awake's hold at the compositor, by its service",
        lambda: ("keep-awake", service) in [(i["kind"], i.get("pid")) for i in inhibitors()],
    )
    assert [(h["name"], h["detail"]) for h in awake()["holders"]] == [("VLC media player", "Playing some media.")], awake()
    panel_shot("cup-keep-awake-and-vlc")
    # Locked by hand, Keep Awake holds the displays on as it always has…
    lock_by_hand("Keep Awake on, VLC playing")
    stays_lit_locked("Keep Awake on, VLC playing")
    # …and off, VLC's hold alone doesn't: the displays' minute runs, and
    # they turn off with VLC playing on behind the lock.
    print(user("aizome-awake off"))
    until("Keep Awake's hold gone", lambda: all(i["kind"] == "app" for i in inhibitors()))
    goes_dark_locked("VLC playing, Keep Awake off again", lambda: playing("vlc"))
    holds_again_unlocked("VLC playing", lambda: playing("vlc"))
    until("VLC's cup again", lambda: awake()["cup"] == "apps")

    let_go = said("vlc", LET_GO)
    mpris("vlc", "call", "Pause")
    until("VLC paused", lambda: not playing("vlc"))
    until("VLC gave its cookie back", lambda: said("vlc", LET_GO) > let_go)
    cup_goes("VLC paused")
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

with subtest("Chromium playing a page's video holds idleness off, in a window and full screen, and paused lets it come"):
    assert desktop_exec("chromium-browser") == "Exec=chromium %U", desktop_exec("chromium-browser")
    user(f"aizomectl spawn {CHROMIUM} {PAGE}")
    until("the page", lambda: page().startswith("paused"), timeout=120)
    # A Wayland client of its own, as it starts in an Aizome session.
    chromium = until("Chromium's window", lambda: window("chromium-browser"))
    assert machine.succeed(f"cat /proc/{chromium['pid']}/comm").strip() == "chromium", chromium
    press("p")
    until("Chromium playing", page_playing)
    stays_awake("Chromium playing")
    assert page_playing(), "Chromium stopped playing"
    # With the keys it holds both ways, its window's inhibitor and the
    # bus under its program's path: one row, under its name.
    chromium_holds = [i.get("window") for i in inhibitors() if i.get("app_id") == "chromium-browser"]
    assert chromium_holds == [chromium["id"]], inhibitors()
    assert said(CHROMIUM_NAME, HELD) > said(CHROMIUM_NAME, LET_GO), "Chromium with the keys didn't ask the bus as well"
    cup_lists([("Chromium", "Video Wake Lock")], "cup-chromium")
    press("f")
    until("Chromium full screen", lambda: (w := window("chromium-browser")) and w["fullscreen"] and "fullscreen" in w["title"])
    until("Chromium playing full screen", page_playing)
    stays_awake("Chromium playing full screen")
    assert page_playing(), "Chromium stopped playing"
    machine.screenshot("chromium-playing")
    press("p")
    until("Chromium paused", lambda: page().startswith("paused"))
    goes_idle("Chromium paused")
    wake()

    # A video that starts by itself while the person is in another window,
    # as the next in a playlist does: Chromium takes its Wayland inhibitor
    # only for a window with the keyboard or the pointer, so here the
    # screen saver is all that holds.
    press("f")
    until("Chromium out of full screen", lambda: (w := window("chromium-browser")) and not w["fullscreen"])
    held, let_go = said(CHROMIUM_NAME, HELD), said(CHROMIUM_NAME, LET_GO)
    # The page plays twenty seconds after this key.
    press("d")
    asked = time.monotonic()
    user("aizomectl spawn foot")
    foot = until("another window", lambda: window("foot"))
    middle = f"{foot['geometry']['x'] + foot['geometry']['w'] // 2} {foot['geometry']['y'] + foot['geometry']['h'] // 2}"
    user(f"aizomectl focus {foot['id']} && aizomectl input motion {middle}")
    focus = ipc("input-state")["state"]
    assert (focus["keyboard_focus"], focus["pointer_focus"]) == ("foot", "foot"), focus
    # The other window had both before the video started, or this would be
    # the case above again, passing on Chromium's own inhibitor.
    elsewhere = time.monotonic() - asked
    still = page()
    assert elsewhere < 15 and still.startswith("paused"), (
        f"the video may have started with Chromium in use: {elsewhere:.1f} s after the key the page said {still!r}"
    )
    until("Chromium playing by itself", page_playing, timeout=40)
    started = time.monotonic() - asked
    focus = ipc("input-state")["state"]
    assert (focus["keyboard_focus"], focus["pointer_focus"]) == ("foot", "foot"), focus
    print(
        f"Chromium's video started by itself: the other window had the keyboard and the pointer {elsewhere:.1f} s "
        f"after the key, with the page still saying {still!r}; it was seen playing {started:.1f} s after the key"
    )
    stays_awake("Chromium playing a video that started by itself")
    assert page_playing(), "Chromium stopped playing"
    assert said(CHROMIUM_NAME, HELD) > held, "Chromium played on without asking the shell's screen saver"
    press("p")
    until("Chromium paused", lambda: page().startswith("paused"))
    until("Chromium gave its cookie back", lambda: said(CHROMIUM_NAME, LET_GO) > let_go)
    close("foot")
    close("chromium-browser")
    # Gone altogether: a second Chromium would only open a window in the first.
    machine.wait_until_fails("pgrep -u alice -x chromium", timeout=30)

with subtest("Chromium as an X11 client has only the screen saver to ask, and it holds idleness off"):
    held, let_go = said(CHROMIUM_NAME, HELD), said(CHROMIUM_NAME, LET_GO)
    user(f"aizomectl spawn {CHROMIUM} --ozone-platform=x11 {PAGE}")
    until("the page", lambda: page().startswith("paused"), timeout=120)
    chromium = until("Chromium's window", lambda: window("chromium-browser"))
    assert "xwayland-satel" in machine.succeed(f"cat /proc/{chromium['pid']}/comm"), chromium
    press("p")
    until("Chromium playing", page_playing)
    stays_awake("Chromium playing as an X11 client")
    assert page_playing(), "Chromium stopped playing"
    assert said(CHROMIUM_NAME, HELD) > held, "Chromium played on without asking the shell's screen saver"
    # Paused, it gives its cookie back, as VLC does: the session going
    # idle after that is VLC's part of this check.
    press("p")
    until("Chromium paused", lambda: page().startswith("paused"))
    until("Chromium gave its cookie back", lambda: said(CHROMIUM_NAME, LET_GO) > let_go)
    close("chromium-browser")

with subtest("an app that asks the desktop portal reaches the same screen saver"):
    # org.freedesktop.portal.Inhibit, for idleness (8): Aizome's sessions
    # answer it with the GTK portal, which asks the session bus's screen
    # saver in turn, with no name for an app that isn't sandboxed. Giving
    # the hold back is the portal's to do when its caller goes. This
    # caller goes at once, and then now and again the hold isn't given
    # back (2 of 30 such callers in a trial; why isn't known), so that
    # isn't asked of it, and this comes last.
    user(
        "busctl --user call org.freedesktop.portal.Desktop /org/freedesktop/portal/desktop "
        "org.freedesktop.portal.Inhibit Inhibit 'sua{sv}' '' 8 1 reason s 'a film'"
    )
    until("the portal's hold", lambda: said("", HELD + ': "a film"') > 0)
    # It gave no name, so its row is the portal's own, with the reason.
    # The portal usually gives the hold back as its caller goes, and the
    # row lingers only a moment after: said if it was seen, not required.
    rows = within(lambda: [h for h in awake()["holders"] if h["detail"] == "a film"], 3)
    if rows:
        print(f"the portal's hold was listed as {rows[0]['name']!r}")
    else:
        print("the portal's hold had gone before its row was looked for")
