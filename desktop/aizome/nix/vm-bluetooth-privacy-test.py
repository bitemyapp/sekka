import re
import time
import tomllib

STATE = "/run/aizome-bluetooth/state.toml"
CONFIG = "/run/aizome-bluetooth/main.conf"
CHOICE = "/var/lib/aizome/bluetooth-privacy"
PAIRINGS = "/var/lib/aizome/bluetooth-pairings.toml"
STORAGE = "/var/lib/bluetooth"
KEYBOARD = "F4:4E:FD:2A:03:6F"

machine.wait_for_unit("multi-user.target")
machine.wait_for_unit("bluetooth.service")


def adapters():
    """The virtual controllers' folders in BlueZ's storage, once it made them."""
    out = machine.succeed(f"ls {STORAGE}")
    return [a for a in out.split() if re.fullmatch(r"([0-9A-F]{2}:){5}[0-9A-F]{2}", a)]


def state():
    return tomllib.loads(machine.succeed(f"cat {STATE}"))


def settings():
    """hci0's current settings, as the kernel has them; none while it can't say."""
    status, out = machine.execute("btmgmt --index 0 info")
    current = re.search(r"current settings: (.*)", out) if status == 0 else None
    return current.group(1).split() if current else None


def private():
    """Whether the kernel has LE privacy on for hci0 (what bluetoothd set).
    The word itself: `ll-privacy` is the controller's address resolution."""
    current = settings()
    assert current is not None, machine.execute("btmgmt --index 0 info")[1]
    return "privacy" in current


def wait_private(on):
    """Until bluetoothd, starting, has set the kernel's LE privacy so."""
    for _ in range(60):
        current = settings()
        if current is not None and ("privacy" in current) == on:
            return
        time.sleep(0.5)
    raise AssertionError(f"privacy never {'on' if on else 'off'}: {settings()}")


def restart_as_root():
    machine.succeed("systemctl restart bluetooth.service")
    machine.wait_for_unit("bluetooth.service")


def start():
    machine.succeed("systemctl start bluetooth.service")
    machine.wait_for_unit("bluetooth.service")


def stop():
    machine.succeed("systemctl stop bluetooth.service")


def pair_keyboard(adapter, key="00112233445566778899AABBCCDDEEFF"):
    """A keyboard's LE pairing in BlueZ's storage, as BlueZ writes one."""
    folder = f"{STORAGE}/{adapter}/{KEYBOARD}"
    lines = ["[General]", "Name=Keychron K3", "AddressType=public", "SupportedTechnologies=LE;", "Trusted=true", ""]
    lines += ["[LongTermKey]", f"Key={key}", "Authenticated=0", "EncSize=16", "EDiv=0", "Rand=0"]
    quoted = " ".join(f"'{line}'" for line in lines)
    machine.succeed(f"mkdir -p '{folder}' && printf '%s\\n' {quoted} > '{folder}/info' && chmod 600 '{folder}/info'")


def forget_keyboard(adapter):
    machine.succeed(f"rm -rf '{STORAGE}/{adapter}/{KEYBOARD}'")


def helper_as(user, *args, ok=True):
    run = machine.succeed if ok else machine.fail
    out = run(f"su - {user} -c '/run/wrappers/bin/pkexec --disable-internal-agent {helper} {' '.join(args)}' 2>&1")
    print(out)
    return out


with subtest("the policy names the installed helper, and pkexec can run it"):
    policy = machine.succeed("cat /run/current-system/sw/share/polkit-1/actions/org.aizome.Bluetooth.policy")
    named = re.search(r'exec\.path">([^<]+)<', policy)
    assert named, policy
    helper = named.group(1)
    assert helper.endswith("/libexec/aizome-bluetooth"), helper
    machine.succeed(f"test -x {helper}")
    machine.succeed("test -u /run/wrappers/bin/pkexec")

with subtest("nothing paired over LE: on, from bluetoothd's start"):
    machine.wait_until_succeeds(f"test -s {STATE}")
    s = state()
    assert (s["privacy"], s["why"], s["choice"]) == (True, "no-pairings", "automatic"), s
    # Where main.conf led when bluetoothd started: Settings compares it.
    assert s["main-conf"] == machine.succeed("realpath /etc/bluetooth/main.conf").strip(), s
    config = machine.succeed(f"cat {CONFIG}")
    assert "\nPrivacy=device\n" in config and config.startswith("[General]\n"), config
    wait_private(True)
    # BlueZ made the computer's key, which only privacy makes.
    machine.wait_until_succeeds(f"ls {STORAGE}/*/identity")
    # Settings reads what it shows as anyone; bluetoothd's copy of main.conf
    # and the pairings kept are root's alone.
    machine.succeed(f"su - bob -c 'cat {STATE} /etc/aizome/bluetooth-privacy.toml'")
    assert machine.succeed(f"stat -c '%a %U' {CONFIG} {PAIRINGS}").split("\n")[:2] == ["600 root", "600 root"]
    machine.fail(f"su - bob -c 'cat {CONFIG}'")

a, b = adapters()[:2]

with subtest("first time Aizome chooses: one adapter's key says nothing of another's keyboard"):
    stop()
    # As before Aizome kept count: adapter a has BlueZ's key, b a keyboard.
    machine.succeed(f"rm -f {PAIRINGS} '{STORAGE}/{b}/identity'")
    pair_keyboard(b)
    start()
    s = state()
    assert (s["privacy"], s["why"]) == (False, "earlier-pairings"), s
    assert "\nPrivacy=off\n" in machine.succeed(f"cat {CONFIG}")
    wait_private(False)
    machine.succeed(
        "journalctl -b -u bluetooth.service | grep -q 'LE privacy off: a device paired over LE may not have the key'"
    )

with subtest("a random static adapter's keyboard counts too"):
    stop()
    machine.succeed(f"rm -f {PAIRINGS}")
    forget_keyboard(b)
    pair_keyboard(f"static-{b}")
    start()
    assert (state()["privacy"], state()["why"]) == (False, "earlier-pairings"), state()
    stop()
    machine.succeed(f"rm -rf '{STORAGE}/static-{b}'")
    start()
    assert (state()["privacy"], state()["why"]) == (True, "no-pairings"), state()
    wait_private(True)

with subtest("without polkit's yes, nothing changes"):
    helper_as("bob", "set", "on", ok=False)
    machine.fail(f"test -e {CHOICE}")
    helper_as("bob", "restart", ok=False)

with subtest("bluetoothd's start alone prepares what it reads"):
    out = helper_as("alice", "prepare", ok=False)
    assert "bluetoothd's start prepares it" in out, out

with subtest("a keyboard paired while privacy was chosen off keeps automatic off"):
    helper_as("alice", "set", "off")
    restart_as_root()
    assert (state()["privacy"], state()["why"]) == (False, "chosen")
    wait_private(False)
    # Paired meanwhile, on the adapter that has BlueZ's key from before.
    pair_keyboard(a)
    helper_as("alice", "set", "automatic")
    machine.fail(f"test -e {CHOICE}")
    restart_as_root()
    assert (state()["privacy"], state()["why"]) == (False, "earlier-pairings"), state()

with subtest("turned on in Settings: kept, and on from the restart asked for"):
    pid = machine.succeed("systemctl show -p MainPID --value bluetooth.service").strip()
    helper_as("alice", "set", "on")
    assert machine.succeed(f"cat {CHOICE}").strip() == "on"
    # Not yet: bluetoothd runs as it started.
    assert state()["privacy"] is False and not private()
    assert machine.succeed("systemctl show -p MainPID --value bluetooth.service").strip() == pid
    helper_as("alice", "restart")
    machine.wait_for_unit("bluetooth.service")
    s = state()
    assert (s["privacy"], s["why"], s["choice"]) == (True, "chosen", "on"), s
    wait_private(True)
    assert machine.succeed("systemctl show -p MainPID --value bluetooth.service").strip() != pid

with subtest("the keyboard forgotten and paired again with privacy on: automatic stays on"):
    forget_keyboard(a)
    restart_as_root()
    pair_keyboard(a, key="FFEEDDCCBBAA99887766554433221100")
    helper_as("alice", "set", "automatic")
    restart_as_root()
    assert (state()["privacy"], state()["why"]) == (True, "all-have-key"), state()
    wait_private(True)

with subtest("a main.conf prepare can't read: bluetoothd reads it as it is"):
    stop()
    # A Latin-1 comment, which GLib passes over; LE only, to tell it's read.
    machine.succeed(
        "printf '# caf\\351\\n[General]\\nControllerMode=le\\nPrivacy=off\\n' > /root/latin1.conf && "
        "mount --bind /root/latin1.conf \"$(realpath /etc/bluetooth/main.conf)\""
    )
    start()
    assert state()["why"] == "unprepared", state()
    machine.fail(f"test -e {CONFIG}")
    machine.succeed("journalctl -b -u bluetooth.service | grep -q 'not UTF-8'")
    wait_private(False)
    machine.wait_until_succeeds("btmgmt --index 0 info | grep 'current settings:' | grep -vq 'br/edr'", timeout=30)
    stop()
    machine.succeed("umount \"$(realpath /etc/bluetooth/main.conf)\"")
    start()
    # Pairings made while it read the system's count as without the key;
    # the keyboard from before has it still.
    assert (state()["privacy"], state()["why"]) == (True, "all-have-key"), state()
