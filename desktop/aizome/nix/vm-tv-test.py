"""The login screen and the TV account, from a game controller alone.

The controller changes nothing on the login screen until it's pressed;
its first press only shows what it does. It then chooses Alice and types
her password on the on-screen keyboard, and her desktop starts. The TV
account, made by Settings' helper with a PIN, is chosen with the
controller and its PIN pressed on the PIN pad; its session is the
compositor in kiosk mode with Chanoma alone, which the controller sets up
for the first time (a grown-up's PIN, then kids' mode). The drive it
opens Alice may read but not change, then and after it logs out. Log Out
ends it all and the login screen comes back, the TV account's PIN pad
ready.
"""

import json
import re
import shlex
import time

INSPECT = "/run/greeter-inspect"
TV_PIN = "2468"


def until(description, check, timeout=90):
    deadline = time.monotonic() + timeout
    last = None
    while time.monotonic() < deadline:
        try:
            last = check()
        except Exception as e:  # noqa: BLE001 - the check is retried
            last = e
        if last and not isinstance(last, Exception):
            return last
        time.sleep(0.5)
    machine.screenshot("failed")
    machine.execute("journalctl --no-pager > /tmp/failed-tv.log")
    machine.copy_from_machine("/tmp/failed-tv.log")
    raise AssertionError(f"{description}: {last!r}; the greeter shows {texts()}")


def docs():
    # The running greeter's alone: one before it leaves its files behind.
    pid = machine.succeed("pgrep -u greeter -x aizome-greeter || true").strip().split("\n")[0]
    if not pid:
        return []
    out = machine.succeed(f'for f in {INSPECT}/*.{pid}.*.json; do [ -f "$f" ] && cat "$f" && echo; done; true')
    return [json.loads(line) for line in out.splitlines() if line.strip()]


def texts():
    try:
        return [e["text"] for d in docs() for e in d.get("elements", []) if e.get("text") and e.get("visible", True)]
    except Exception:  # noqa: BLE001 - between two greeters
        return []


def greeter_log():
    return machine.succeed("journalctl -t aizome-greeter --no-pager -o cat")


def session_log():
    # The compositor colours its log's field names; without the colours.
    return re.sub(r"\x1b\[[0-9;]*m", "", machine.succeed("journalctl -t chanoma-session --no-pager -o cat"))


def greeter_up():
    return machine.execute("pgrep -u greeter -x aizome-greeter")[0] == 0


def user_units(user, args):
    """What `user`'s own systemd says of `args`."""
    return machine.succeed(
        f"runuser -u {user} -- env XDG_RUNTIME_DIR=/run/user/$(id -u {user}) systemctl --user {args}"
    ).strip()


def pad(command):
    machine.succeed(f"echo {shlex.quote(command)} > /run/tv-pad")
    time.sleep(0.6)


def shows(text):
    return until(f"{text!r} on the login screen", lambda: any(t == text for t in texts()))


start_all()
machine.wait_for_unit("multi-user.target")

with subtest("Settings' helper makes the TV account, with a PIN"):
    machine.succeed(f"printf %s {TV_PIN} | /etc/tv-account-helper set pin")
    record = machine.succeed("cat /var/lib/aizome/tv-account.toml")
    assert 'user = "tv"' in record and 'auth = "pin"' in record and "pin_length = 4" in record, record
    assert TV_PIN not in record, "the PIN isn't written down"
    passwd = machine.succeed("getent passwd tv")
    assert passwd.strip().endswith("/nologin"), passwd
    # A console won't let it in.
    machine.fail("su tv -c true </dev/null")
    # The login screen reads it when it starts.
    machine.succeed("systemctl restart greetd")
    until("the login screen", greeter_up)
    shows("Alice Kimura")
    shows("TV")
    machine.screenshot("tv-greeter-untouched")

with subtest("a controller changes nothing until it's pressed"):
    # The clock may turn over meanwhile.
    still = lambda: [t for t in texts() if not re.match(r"^\d{1,2}:\d{2}", t)]
    before = still()
    machine.succeed("systemd-run --unit tv-pad /etc/tv-pad")
    machine.wait_for_file("/run/tv-pad")
    time.sleep(2)
    assert "first press" not in greeter_log(), greeter_log()
    assert still() == before, (before, still())
    pad("tap south")
    until("the first press", lambda: "a controller's first press" in greeter_log())
    # It only showed what the controller does: Alice is still to pick.
    assert not any(t.startswith("Session:") for t in texts()), texts()
    machine.screenshot("tv-greeter-controller")

with subtest("the controller types Alice's password on the on-screen keyboard"):
    pad("tap south")
    shows("Session: Aizome")
    # q, where the keyboard starts; then w, e, r along the row; Start.
    pad("tap south")
    for _ in range(3):
        pad("tap right")
        pad("tap south")
    machine.screenshot("tv-greeter-keyboard")
    pad("tap start")
    until("Alice's desktop", lambda: machine.execute("pgrep -u alice -x aizome")[0] == 0)
    until(
        "Alice's login starting what every account's does",
        lambda: user_units("alice", "is-active desktop-errand.service") == "active",
    )
    machine.succeed("loginctl terminate-user alice")
    until("the login screen again", greeter_up, timeout=60)

with subtest("the TV account's PIN, pressed on the PIN pad"):
    # Alice's password is asked again; the controller puts the keyboard
    # away and goes back to the people, then to the TV.
    shows("Alice Kimura")
    time.sleep(1)
    pad("tap south")
    pad("tap east")
    pad("tap east")
    shows("TV")
    pad("tap right")
    pad("tap south")
    machine.screenshot("tv-greeter-pin")
    # 2, 4, 6 and 8: Down, Right, RB and X. It goes at the fourth.
    for b in ["down", "right", "rb", "x"]:
        pad(f"tap {b}")
    until("Chanoma, as the TV account", lambda: machine.execute("pgrep -u tv -f aizome-chanoma")[0] == 0)
    until("the login screen gone", lambda: not greeter_up())

with subtest("the TV account's session is Chanoma alone"):
    until("Chanoma holds the controller", lambda: "is player 1" in session_log())
    # The compositor knows the input devices (and a laptop's lid) as it
    # starts, before any key or movement: nothing has been pressed since.
    until("the keyboard known at once", lambda: "keyboard device=" in session_log(), timeout=20)
    assert machine.execute("pgrep -u tv -f aizome-shell")[0] != 0, "no shell"
    assert machine.execute("pgrep -u tv -x xwayland-satellite")[0] != 0, "no X11 bridge"
    # Its systemd starts the bus and sound, not what every account's does.
    print(user_units("tv", "list-units --state=active --no-legend --plain"))
    assert user_units("tv", "show -p LoadState --value desktop-errand.service") == "masked"
    until(
        "the TV account's bus and sound",
        lambda: user_units("tv", "is-active dbus.socket pipewire.socket pipewire-pulse.socket").split() == ["active"] * 3,
    )
    time.sleep(2)
    machine.screenshot("tv-chanoma-welcome")

with subtest("the controller sets Chanoma up, and it's kids' mode"):
    pad("tap south")
    time.sleep(1)
    for _ in range(2):
        for _ in range(4):
            pad("tap up")
        pad("tap south")
    until("the grown-ups' settings", lambda: "chanoma: GrownUps" in session_log())
    pad("tap east")
    until("Home, for the kids", lambda: "chanoma: Home" in session_log())
    time.sleep(1)
    machine.screenshot("tv-chanoma-kids")


def alice_reads_the_stick():
    # Made at boot for every account to look in, not by UDisks for the TV
    # account alone.
    assert machine.succeed("stat -c %a /run/media/tv").strip() == "755"
    assert machine.succeed("runuser -u alice -- cat /run/media/tv/TVSTICK/hello.txt").strip() == "hello"
    machine.fail("runuser -u alice -- touch /run/media/tv/TVSTICK/alice.txt")
    # -f, or rm asks first about a file it may not write.
    machine.fail("runuser -u alice -- rm -f /run/media/tv/TVSTICK/hello.txt")
    machine.succeed("test -f /run/media/tv/TVSTICK/hello.txt")


with subtest("the drive the TV account opened, Alice may read but not change"):
    until("the stick opened", lambda: "chanoma: opened the drive TVSTICK at /run/media/tv/TVSTICK" in session_log())
    alice_reads_the_stick()
    # The TV account itself still may.
    machine.succeed("runuser -u tv -- touch /run/media/tv/TVSTICK/tv.txt")

with subtest("Log Out ends the session and the login screen comes back"):
    for _ in range(5):
        pad("tap down")
    pad("tap right")
    pad("tap south")
    until("the TV account's session ended", lambda: machine.execute("pgrep -u tv -f aizome-chanoma")[0] != 0)
    until("the login screen again", greeter_up, timeout=60)
    until("no compositor of the TV's", lambda: machine.execute("pgrep -u tv -x aizome")[0] != 0)
    # The stick stays open where the TV account opened it, and readable.
    machine.succeed("findmnt -n /run/media/tv/TVSTICK")
    alice_reads_the_stick()
    shows("TV")
    # The TV was last: its PIN is asked for, on the pad when a controller
    # presses.
    time.sleep(1)
    pad("tap south")
    until("the controller wakes the new login screen", lambda: greeter_log().count("a controller's first press") >= 3)
    machine.screenshot("tv-greeter-after")
