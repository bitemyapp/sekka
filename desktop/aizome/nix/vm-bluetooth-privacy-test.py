import re
import time
import tomllib

STATE = "/run/aizome-bluetooth/state.toml"
CONFIG = "/run/aizome-bluetooth/main.conf"
CHOICE = "/var/lib/aizome/bluetooth-privacy"

machine.wait_for_unit("multi-user.target")
machine.wait_for_unit("bluetooth.service")


def adapters():
    """The virtual controllers' folders in BlueZ's storage, once it made them."""
    out = machine.succeed("ls /var/lib/bluetooth")
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
    machine.wait_until_succeeds("ls /var/lib/bluetooth/*/identity")
    # Settings reads it all as anyone.
    machine.succeed(f"su - bob -c 'cat {STATE} /etc/aizome/bluetooth-privacy.toml'")

with subtest("a keyboard paired over LE while it was off: off, so it keeps connecting"):
    machine.succeed("systemctl stop bluetooth.service")
    adapter = adapters()[0]
    machine.succeed("rm -f /var/lib/bluetooth/*/identity")
    machine.succeed(
        f"mkdir -p '/var/lib/bluetooth/{adapter}/F4:4E:FD:2A:03:6F' && "
        f"printf '%s\\n' '[General]' 'Name=Keychron K3' 'AddressType=public' 'SupportedTechnologies=LE;' "
        f"'Trusted=true' '' '[LongTermKey]' 'Key=00112233445566778899AABBCCDDEEFF' 'Authenticated=0' "
        f"'EncSize=16' 'EDiv=0' 'Rand=0' > '/var/lib/bluetooth/{adapter}/F4:4E:FD:2A:03:6F/info' && "
        f"chmod 600 '/var/lib/bluetooth/{adapter}/F4:4E:FD:2A:03:6F/info'"
    )
    machine.succeed("systemctl start bluetooth.service")
    machine.wait_for_unit("bluetooth.service")
    s = state()
    assert (s["privacy"], s["why"]) == (False, "earlier-pairings"), s
    assert "\nPrivacy=off\n" in machine.succeed(f"cat {CONFIG}")
    wait_private(False)
    machine.succeed("journalctl -b -u bluetooth.service | grep -q 'LE privacy off: devices were paired over LE while it was off'")

with subtest("without polkit's yes, nothing changes"):
    helper_as("bob", "set", "on", ok=False)
    machine.fail(f"test -e {CHOICE}")
    helper_as("bob", "restart", ok=False)

with subtest("bluetoothd's start alone prepares what it reads"):
    out = helper_as("alice", "prepare", ok=False)
    assert "bluetoothd's start prepares it" in out, out

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

with subtest("turned off: off from the next start; automatic again: chosen for the computer"):
    helper_as("alice", "set", "off")
    restart_as_root()
    assert (state()["privacy"], state()["why"]) == (False, "chosen")
    wait_private(False)
    helper_as("alice", "set", "automatic")
    machine.fail(f"test -e {CHOICE}")
    restart_as_root()
    # BlueZ's key is there from when it was on: devices paired since have it.
    assert (state()["privacy"], state()["why"]) == (True, "already-on"), state()
