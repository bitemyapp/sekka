"""Component regression: Chanoma in a real Aizome session, driven by a
game controller alone (a virtual one, through uinput).

Chanoma starts with the session; the controller is found and held from
other readers; Home's tiles open a game (OpenBIOS linked into the cores'
system folder as it starts); the game hears the controller
(its screen goes dark while Right is held, read from the compositor's own
screenshot); the Home button pauses it under a menu; stopping keeps where
it was left; the controller going and coming back is player 1 again. A
page in the browser has a way home without the controller too: Home or F1
on a keyboard (which the compositor keeps from the page) and the bar the
pointer brings down from the top of the screen; it stays on its site, a
link or a script elsewhere refused with the page left where it was; and a
box chosen on it brings Chanoma's keyboard, which the controller types
with. The PIN is pressed on the controller, the pad the same whichever
button went. Two USB sticks plugged in from the start: the one that can be
is opened as Chanoma starts, and the one that can't (exFAT, its module not
loaded) is said once, not again at the next start. Chanoma crashing
(SIGSEGV) starts again on a page saying so, which goes Home, and leaves no
core dump. This fixture renders in software (shared-memory frames, no
VA-API).
"""

import hashlib
import json
import os
import shlex
import time

APP = "org.aizome.Chanoma"
PIN = "2468"
GAME_KEY = "gb/gb/Chanoma Test (World).zip"


def user(command):
    return machine.succeed(
        "su - alice -c "
        + shlex.quote(
            "export XDG_RUNTIME_DIR=/run/user/1000; "
            "export WAYLAND_DISPLAY=$(ls /run/user/1000 | grep -m1 '^wayland-[0-9]*$'); "
            "export AIZOME_SOCKET=/run/user/1000/aizome.$WAYLAND_DISPLAY.sock; "
            "export DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus; "
            + command
        )
    )


def until(description, check, timeout=60):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        result = check()
        if result:
            return result
        time.sleep(0.25)
    machine.screenshot("failed")
    machine.succeed("journalctl _UID=1000 --no-pager > /tmp/failed-chanoma.log")
    machine.copy_from_machine("/tmp/failed-chanoma.log")
    raise AssertionError(description)


def journal():
    return machine.succeed("journalctl _UID=1000 --no-pager -o cat")


def logged(text, times=1):
    return journal().count(text) >= times


def window(app=APP):
    return next(
        (w for w in json.loads(user("aizomectl --json windows"))["windows"] if w["app_id"] == app),
        None,
    )


def pad(command):
    machine.succeed(f"echo {shlex.quote(command)} > /run/chanoma-pad")
    # The pad answers once it has done it (taps take a third of a second).
    time.sleep(0.5)


def brightness(box):
    """The mean brightness (0–255) of a part of the screen, as the
    compositor captures it."""
    user("aizomectl screenshot /tmp/chanoma-shot.png")
    left, top, right, bottom = box
    return float(
        machine.succeed(
            "python3 -c "
            + shlex.quote(
                "from PIL import Image, ImageStat; "
                "im = Image.open('/tmp/chanoma-shot.png').convert('L')"
                f".crop(({left}, {top}, {right}, {bottom})); "
                "print(ImageStat.Stat(im).mean[0])"
            )
        ).strip()
    )


start_all()
machine.wait_for_unit("multi-user.target")
machine.wait_until_succeeds(
    "su - alice -c "
    + shlex.quote(
        "export XDG_RUNTIME_DIR=/run/user/1000; "
        "export WAYLAND_DISPLAY=$(ls /run/user/1000 | grep -m1 '^wayland-[0-9]*$'); "
        "AIZOME_SOCKET=/run/user/1000/aizome.$WAYLAND_DISPLAY.sock aizomectl windows"
    ),
    timeout=120,
)
until("Chanoma starts with the session", lambda: window())
# It waited for the compositor rather than failing and starting again.
assert "the Wayland compositor couldn't be reached" not in journal(), journal()

with subtest("the household is set up"):
    salt = os.urandom(16)
    settings = {
        "pin": {"salt": salt.hex(), "hash": hashlib.pbkdf2_hmac("sha256", PIN.encode(), salt, 1000).hex(), "rounds": 1000},
        "pin_length": 4,
        "services": ["pbs-kids"],
        "game_folders": ["/etc/chanoma-games"],
        "video_folders": [],
        "games_allowed": [GAME_KEY],
        "custom_services": [{"name": "Test Site", "url": "http://127.0.0.1:8000/"}],
        "setup_done": True,
    }
    machine.succeed("mkdir -p /home/alice/.config/aizome")
    machine.succeed(
        "cat > /home/alice/.config/aizome/chanoma.json <<'EOF'\n" + json.dumps(settings) + "\nEOF"
    )
    machine.succeed("chown -R alice /home/alice/.config/aizome")
    user("systemctl --user restart aizome-chanoma")
    until("Chanoma comes back full screen", lambda: (w := window()) and w["fullscreen"])

with subtest("the drives plugged in are opened, and one that can't be is said once"):
    # Chanoma has started twice: with the session, and set up just now.
    until("STICK opened", lambda: logged("chanoma: opened the drive STICK at /run/media/alice/STICK"))
    until("BROKEN tried at each start", lambda: logged("chanoma: couldn't open the drive BROKEN", 2))
    j = journal()
    assert j.count("chanoma: opened the drive STICK") == 1, "mounted already at the second start"
    assert j.count("chanoma: said BROKEN couldn't be opened") == 1, j
    machine.succeed("findmnt -n /run/media/alice/STICK")
    machine.succeed("test -f '/run/media/alice/STICK/gb/Stick Test (World).gb'")
    warned = json.loads(machine.succeed("cat /home/alice/.local/state/aizome/chanoma/drives.json"))
    assert len(warned["drives"]) == 1, warned

with subtest("the controller is found and held"):
    machine.succeed("systemd-run --unit chanoma-pad /etc/chanoma-pad")
    machine.wait_for_file("/run/chanoma-pad")
    until("Chanoma finds the controller", lambda: logged("controller Xbox Wireless Controller"))
    assert logged("is player 1"), journal()
    # Held: another reader can't take it.
    machine.fail(
        "python3 -c "
        + shlex.quote(
            "import evdev; "
            "d = next(evdev.InputDevice(p) for p in evdev.list_devices() "
            "if evdev.InputDevice(p).name == 'Xbox Wireless Controller'); d.grab()"
        )
    )

with subtest("the controller opens a game from Home"):
    # Home's shelves: Watch, Games, More.
    pad("tap down")
    pad("tap south")
    until("the game plays", lambda: logged("chanoma: Playing"))
    # The PlayStation's BIOS the module offers (OpenBIOS) is linked into
    # the cores' system folder as a game starts.
    bios = "/home/alice/.local/share/aizome/chanoma/system/openbios.bin"
    machine.succeed(f"test -L {bios} && test $(stat -L -c %s {bios}) = 524288")
    # The game is zipped: unpacked for the core.
    machine.succeed("test -s '/home/alice/.cache/aizome/chanoma/unzipped/'*'/Chanoma Test (World).gb'")
    # The databases the module installs were read: the game named as the
    # Game Boy's database names one is found there (the test game isn't).
    assert logged("1 of 2 games found in the databases by name"), journal()
    time.sleep(2)
    # The picture's top left, clear of the logo the game boots with.
    box = (260, 60, 460, 260)
    light = brightness(box)
    pad("down right")
    time.sleep(1)
    dark = brightness(box)
    pad("up right")
    time.sleep(1)
    again = brightness(box)
    print(f"the game's screen: {light:.0f}, {dark:.0f} with Right held, {again:.0f} after")
    assert dark < light * 0.6, (light, dark)
    assert again > dark * 1.5, (dark, again)

with subtest("Home pauses it and stopping keeps where it was"):
    pad("tap guide")
    until("the menu opens", lambda: logged("the Game menu"))
    pad("tap down")
    pad("tap down")
    pad("tap south")
    until("Home again", lambda: logged("chanoma: Home"))
    machine.succeed(f"test -s '/home/alice/.local/share/aizome/chanoma/states/{GAME_KEY.replace('/', '~')}.state'")
    machine.succeed(f"test -s '/home/alice/.local/share/aizome/chanoma/states/{GAME_KEY.replace('/', '~')}.png'")

with subtest("a controller that comes back is player 1 again"):
    pad("unplug")
    until("Chanoma sees it go", lambda: logged("went (player 1)"))
    pad("plug")
    until("and come back", lambda: logged("is player 1", 2))

def browser():
    # Not any other window: Aizome's welcome tour opens at a first login.
    return next(
        (w for w in json.loads(user("aizomectl --json windows"))["windows"] if "chrom" in w["app_id"]),
        None,
    )


with subtest("a page in the browser, driven by the controller"):
    # Home: Keep playing (the game), then Watch: PBS KIDS, Test Site.
    pad("tap down")
    pad("tap right")
    pad("tap south")
    until("the page opens full screen", lambda: (w := browser()) and w["fullscreen"] and w["focused"], timeout=90)
    until("the page loads", lambda: "Chanoma test page" in browser()["title"], timeout=60)
    # The confirm button clicks where the pointer is: the middle, on the
    # page's button.
    pad("tap south")
    until("the click reaches the page", lambda: "clicked" in browser()["title"])
    # The arrows are arrow keys.
    pad("tap right")
    until("the arrow reaches the page", lambda: "key ArrowRight" in browser()["title"])
    # The stick moves the pointer.
    pad("stick 65535 32768")
    time.sleep(0.6)
    pad("stick 32768 32768")
    until("the pointer moves", lambda: "moved" in browser()["title"])
    # Home: the overlay's menu over the page, then the way home.
    pad("tap guide")
    until("the menu opens over the page", lambda: logged("the Page menu"))
    time.sleep(1)
    over = brightness((560, 330, 720, 470))
    print(f"the page under the menu: {over:.0f}")
    assert over < 160, over
    pad("tap down")
    pad("tap south")
    until("home again, the browser closed", lambda: logged("chanoma: Home", 2) and browser() is None, timeout=30)


def count(text):
    return journal().count(text)


def titled(start, description, but=None):
    """Wait for the page's title (the window's, which the browser may add
    to) to start with `start` (and not `but`), saying what it was if not."""

    def check():
        title = browser()["title"]
        return title.startswith(start) and not (but and title.startswith(but))

    try:
        until(description, check)
    except AssertionError:
        print("\n".join(l for l in journal().splitlines() if "keyboard" in l or "DevTools" in l or "page" in l)[-3000:])
        raise AssertionError(f"{description}: the page's title is {browser()['title']!r}")


def open_test_site():
    # First in Keep playing since it last showed.
    pad("tap south")
    until(
        "the page opens",
        lambda: (w := browser()) and w["fullscreen"] and w["focused"] and "Chanoma test page" in w["title"],
        timeout=60,
    )


with subtest("a page has a way home by keyboard, and by mouse"):
    open_test_site()
    menus, homes = count("the Page menu"), count("chanoma: Home")
    # Home on a keyboard is Chanoma's, not the page's (the compositor keeps
    # it): the menu over the page, which has the keys while it shows.
    until("Chanoma keeps Home over the page", lambda: count("Home on the keyboard is kept over window") >= 2)
    user("aizomectl input key home")
    until("Home on the keyboard opens the menu", lambda: count("the Page menu") > menus)
    user("aizomectl input key escape")
    time.sleep(1)
    assert "key" not in browser()["title"], browser()["title"]
    # Put away, the page has the keys again.
    user("aizomectl input key a")
    until("the page has the keys again", lambda: "key a" in browser()["title"])
    # F1 too; then Down and Return: home.
    user("aizomectl input key f1")
    until("F1 opens the menu", lambda: count("the Page menu") > menus + 1)
    user("aizomectl input key down")
    user("aizomectl input key return")
    until("home by keyboard", lambda: count("chanoma: Home") > homes and browser() is None, timeout=30)

    open_test_site()
    menus, homes = count("the Page menu"), count("chanoma: Home")
    until("Chanoma is ready over the page", lambda: count("Home on the keyboard is kept over window") >= 3)
    time.sleep(1)
    # The pointer pushed up to the top (from the middle, as a hand moves
    # it) brings the bar down; its Home (at the left: the 1920 × 1080
    # design is at two thirds here, 40 points down) is the menu, and the
    # menu's Go home goes home.
    user("aizomectl input move 640 300")
    user("aizomectl input move 640 0")
    until("the bar comes down", lambda: logged("the bar over the page"))
    user("aizomectl input move 144 50")
    user("aizomectl input click")
    until("the bar's Home opens the menu", lambda: count("the Page menu") > menus)
    user("aizomectl input move 640 456")
    user("aizomectl input click")
    until("home by mouse", lambda: count("chanoma: Home") > homes and browser() is None, timeout=30)


with subtest("a page stays on its site"):
    open_test_site()
    refused, menus, homes = count("is outside"), count("the Page menu"), count("chanoma: Home")
    until("Chanoma is ready over the page", lambda: count("Home on the keyboard is kept over window") >= 4)
    # Pages aren't told the browser is steered.
    assert "(automated)" not in browser()["title"], browser()["title"]
    # The link elsewhere, clicked (at the bottom left), then a script
    # going there: both refused, and the page stays as it was.
    user("aizomectl input move 640 400")
    user("aizomectl input move 100 740")
    user("aizomectl input click")
    until("the link elsewhere is refused", lambda: count("is outside") > refused)
    machine.screenshot("page-outside")
    user("aizomectl input key o")
    until("the script going elsewhere is refused", lambda: count("is outside") > refused + 1)
    time.sleep(1)
    assert browser()["title"].startswith("Chanoma test page"), browser()["title"]
    machine.fail("journalctl -u chanoma-test-page --no-pager | grep -q elsewhere")
    # Its own site's pages, as ever.
    user("aizomectl input key i")
    until("the site's own other page", lambda: "inside" in browser()["title"])
    user("aizomectl input key home")
    until("the menu over the page", lambda: count("the Page menu") > menus)
    user("aizomectl input key down")
    user("aizomectl input key return")
    until("home again", lambda: count("chanoma: Home") > homes and browser() is None, timeout=30)


with subtest("the controller types into the page with Chanoma's keyboard"):
    open_test_site()
    menus, homes = count("the Page menu"), count("chanoma: Home")
    until("Chanoma is ready over the page", lambda: count("Home on the keyboard is kept over window") >= 5)
    time.sleep(1)
    # The pointer on the search box (put there by the mouse, as the stick
    # would), then A: a box chosen, and the keyboard comes up.
    user("aizomectl input move 640 300")
    user("aizomectl input move 640 90")
    pad("tap south")
    until("the keyboard comes up over the page", lambda: logged("the keyboard over the page"))
    time.sleep(1)
    machine.screenshot("page-keyboard")
    # A types the key it's on (q, the first letter); X takes it back.
    pad("tap south")
    titled("typed q", "q typed into the page")
    pad("tap x")
    titled("typed", "and taken back", but="typed q")
    # Down to a, along to h, A; up to y, along to i, A.
    pad("tap down")
    for _ in range(5):
        pad("tap right")
    pad("tap south")
    pad("tap up")
    for _ in range(2):
        pad("tap right")
    pad("tap south")
    titled("typed hi", "hi typed into the page")
    # Start is Enter: the search goes, and the keyboard with it.
    pad("tap start")
    until("the search sent", lambda: "searched hi" in browser()["title"])
    until("the keyboard put away", lambda: logged("the keyboard put away"))
    machine.succeed("journalctl -u chanoma-test-page --no-pager | grep -q 'GET /?q=hi '")
    pad("tap guide")
    until("the menu over the page", lambda: count("the Page menu") > menus)
    pad("tap down")
    pad("tap south")
    until("home again", lambda: count("chanoma: Home") > homes and browser() is None, timeout=30)


with subtest("leaving with the session keeps a game too"):
    # Start it again from Keep playing (after the page there), then stop
    # Chanoma as logging out does.
    pad("tap right")
    pad("tap south")
    until("the game plays again", lambda: logged("chanoma: Playing", 2))
    before = machine.succeed(
        f"stat -c %Y '/home/alice/.local/share/aizome/chanoma/states/{GAME_KEY.replace('/', '~')}.state'"
    ).strip()
    time.sleep(1.5)
    user("systemctl --user stop aizome-chanoma")
    after = machine.succeed(
        f"stat -c %Y '/home/alice/.local/share/aizome/chanoma/states/{GAME_KEY.replace('/', '~')}.state'"
    ).strip()
    assert int(after) > int(before), (before, after)


def pad_shot():
    """A digest of the PIN pad as the compositor captures it (the 1920 ×
    1080 design at two thirds, 40 points down)."""
    user("aizomectl screenshot /tmp/chanoma-pin.png")
    return machine.succeed(
        "python3 -c "
        + shlex.quote(
            "import hashlib; from PIL import Image; "
            "im = Image.open('/tmp/chanoma-pin.png').convert('RGB').crop((267, 200, 1013, 600)); "
            "print(hashlib.sha256(im.tobytes()).hexdigest())"
        )
    ).strip()


with subtest("the PIN is the controller's buttons, and nobody sees which"):
    user("systemctl --user start aizome-chanoma")
    until("Chanoma back full screen", lambda: (w := window()) and w["fullscreen"])
    until("with the controller", lambda: logged("is player 1", 3))
    time.sleep(1)
    # Down to the last shelf (More: Grown-ups), then A: the PIN pad.
    for _ in range(4):
        pad("tap down")
    pad("tap south")
    until("the PIN pad", lambda: logged("the PIN pad"))
    time.sleep(1)
    # A press; then it taken back with B and another pressed instead: the
    # pad looks the same whichever button went.
    pad("tap down")
    time.sleep(0.5)
    first = pad_shot()
    pad("tap east")
    pad("tap up")
    time.sleep(0.5)
    machine.screenshot("pin-pad")
    assert pad_shot() == first, "the pad shows which button went"
    # Taken back again, then the PIN, 2468: Down, Right, RB and X.
    pad("tap east")
    for b in ("down", "right", "rb", "x"):
        pad(f"tap {b}")
    until("grown-up mode", lambda: logged("chanoma: grown-up mode"))

with subtest("a crash starts Chanoma again, saying so, with no core dump"):
    homes = count("chanoma: Home")
    # The newest is the child the watcher started (the watcher is older).
    child = machine.succeed("pgrep -n -u alice -f '[.]aizome-chanoma-wrapped'").strip()
    machine.succeed(f"kill -SEGV {child}")
    until("the watcher starts it again", lambda: logged("chanoma: crashed (SIGSEGV)"))
    until(
        "a new Chanoma",
        lambda: machine.execute("pgrep -n -u alice -f '[.]aizome-chanoma-wrapped'")[1].strip() not in ("", child),
    )
    until("back full screen", lambda: (w := window()) and w["fullscreen"])
    until("with the controller", lambda: logged("is player 1", 4))
    time.sleep(1)
    machine.screenshot("started-again")
    # The page saying so, whose button goes Home.
    pad("tap south")
    until("Home from the page", lambda: count("chanoma: Home") > homes)
    dumps = machine.succeed("journalctl --no-pager -o cat | grep -c 'dumped core' || true").strip()
    assert dumps == "0", f"{dumps} core dumps"
