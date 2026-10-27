"""Disposable real LUKS devices. Hako actions use real input; inspection only reads UI.

Seeding/locking between cases is fixture setup, not ISO qualification. The same
script runs both original-source negatives: all starts unlocked, locked skips
that first case to reach the independent passphrase regression.
"""
import json
import shlex
import time
from pathlib import Path

APP = "org.aizome.Hako"
INSPECT = "/run/user/1000/encrypted-inspect"
inputs = []


def user(command):
    return machine.succeed("su - alice -c " + shlex.quote(
        "export XDG_RUNTIME_DIR=/run/user/1000; "
        "export WAYLAND_DISPLAY=$(ls /run/user/1000 | grep -m1 '^wayland-[0-9]*$'); "
        "export AIZOME_SOCKET=/run/user/1000/aizome.$WAYLAND_DISPLAY.sock; " + command))


def until(description, predicate):
    deadline = time.monotonic() + 30
    while time.monotonic() < deadline:
        value = predicate()
        if value:
            return value
        time.sleep(0.2)
    raise AssertionError(description)


def windows(app=APP):
    return [w for w in json.loads(user("aizomectl --json windows"))["windows"] if w["app_id"] == app]


def documents():
    return [json.loads(line) for line in machine.succeed(
        f'for f in {INSPECT}/*.json; do if test -f "$f"; then cat "$f"; printf "\\n"; fi; done'
    ).splitlines() if line.strip()]


def elements():
    ws = windows()
    return [(w, e) for w in ws for d in documents()
            if d.get("app_id") == APP and d.get("pid") == w["pid"] and d.get("kind") == "window"
            for e in d["elements"] if e["visible"]]


def find(**attrs):
    return next(((w, e) for w, e in elements() if all(e.get(k) == v for k, v in attrs.items())), None)


def input_(command):
    inputs.append(command)
    user("aizomectl input " + command)
    time.sleep(0.2)


def click(pair):
    w, e = pair
    x, y, width, height = e["rect"]
    g = w["geometry"]
    input_(f"motion {g['x'] + x + width / 2} {g['y'] + y + height / 2}")
    input_("click")


def click_label(label):
    click(until(label, lambda: find(label=label, role="tab")))


def authenticate_if_needed():
    for _ in range(30):
        if any(any(e.get("name") == "auth-password" and e["visible"] for e in d["elements"]) for d in documents()):
            machine.screenshot("authentication")
            input_("type test-only")
            input_("key Return")
            return
        time.sleep(0.1)


def launch_hako():
    user("aizomectl spawn aizome-hako")
    until("Hako opens", lambda: windows())
    until("Hako home contents", lambda: find(label="Home"))


def quit_hako():
    w = windows()[0]
    g = w["geometry"]
    input_(f"motion {g['x'] + 30} {g['y'] - 12}")
    input_("click")
    input_("chord super+q")
    until("Hako quits", lambda: not windows())


def seed(serial, label):
    links = machine.succeed("find /dev/disk/by-id -maxdepth 1 -name " + shlex.quote("*" + serial + "*") + " ! -name '*part*' -print").splitlines()
    assert len(links) == 1, links
    device = machine.succeed("readlink -f " + shlex.quote(links[0])).strip()
    assert device.startswith(("/dev/sd", "/dev/vd")) and "\n" not in device, device
    assert machine.succeed("blockdev --getsize64 " + device).strip() == "268435456"
    assert device != machine.succeed("findmnt -n -o SOURCE /").strip()
    machine.succeed("printf 'label: gpt\\n,200M,L\\n' | sfdisk " + device)
    machine.succeed("udevadm settle")
    part = device + "1"
    machine.wait_until_succeeds("test -b " + part)
    machine.succeed("cryptsetup luksFormat --type luks2 --batch-mode --pbkdf pbkdf2 --iter-time 10 --key-file /etc/aizome-encryption-passphrase " + part)
    machine.succeed("udevadm settle")
    machine.wait_until_succeeds("udisksctl info -b " + part + " | grep -q org.freedesktop.UDisks2.Encrypted:")
    machine.succeed("udisksctl unlock -b " + part + " --key-file /etc/aizome-encryption-passphrase")
    uuid = machine.succeed("cryptsetup luksUUID " + part).strip()
    clear = "/dev/mapper/luks-" + uuid
    machine.wait_until_succeeds("test -b " + clear)
    machine.succeed("mkfs.ext4 -F -L " + label + " " + clear)
    machine.succeed("udevadm settle")
    return part, clear


machine.start()
machine.wait_until_succeeds("pgrep -u alice -x aizome-shell", timeout=120)
try:
    tour = until("first-run tour", lambda: windows("org.aizome.Annai"))[0]
    g = tour["geometry"]
    input_(f"motion {g['x'] + 30} {g['y'] - 12}")
    input_("click")
    input_("chord super+q")
    until("tour closed", lambda: not windows("org.aizome.Annai"))
    machine.succeed("test \"$(cat /etc/aizome-encryption-test-vm)\" = disposable-encryption-test")
    machine.succeed("systemctl start udisks2")
    machine.succeed("udevadm settle")
    visible, clear = seed("QA_VISIBLE", "QA_PRIVATE")
    seed("QA_INTERNAL", "QA_SYSTEM")
    seed("QA_IGNORED", "QA_IGNORED")
    point = "/run/media/alice/QA_PRIVATE"
    machine.succeed("mkdir -p " + point)
    machine.succeed("mount " + clear + " " + point)
    machine.succeed("chown 1000:users " + point)
    machine.succeed("printf 'private fixture content\\n' > " + point + "/encrypted.txt")
    machine.succeed("udevadm settle")
    case = machine.succeed("cat /etc/aizome-encryption-case").strip()
    if case == "all":
        with subtest("unlocked USB keeps identity and hidden ancestors stay hidden"):
            launch_hako()
            machine.screenshot("01-unlocked-sidebar")
            click_label("QA_PRIVATE")
            machine.wait_for_text(r"encrypted\.txt", timeout=30)
            machine.screenshot("02-unlocked-content")
            assert not find(label="QA_SYSTEM") and not find(label="QA_IGNORED")
            machine.succeed("aizome-encrypted-volumes-fixture --ignored --nocapture > /tmp/encrypted-api.log 2>&1")
            quit_hako()
    with subtest("locked volume offers cancel and wrong-passphrase recovery"):
        machine.succeed("umount " + point)
        machine.succeed("rmdir " + point)
        machine.succeed("udisksctl lock -b " + visible)
        machine.wait_until_succeeds("test ! -e " + clear)
        launch_hako()
        locked = until("locked volume in sidebar", lambda: next(((w, e) for w, e in elements()
                        if "Encrypted" in e.get("label", "") and e.get("role") == "tab"), None))
        machine.screenshot("03-locked-sidebar")
        click(locked)
        until("native passphrase prompt", lambda: find(name="unlock-pass"))
        input_("key Return")
        assert find(name="unlock-pass"), "empty passphrase must not submit"
        machine.succeed("test ! -e " + clear)
        machine.screenshot("04-passphrase-prompt")
        input_("type cancelled-secret")
        input_("key Escape")
        until("cancel closes prompt", lambda: not find(name="unlock-pass"))
        machine.succeed("test ! -e " + clear)
        machine.screenshot("05-cancel-keeps-locked")
        click(locked)
        until("retry passphrase prompt", lambda: find(name="unlock-pass"))
        input_("type wrong-passphrase")
        input_("key Return")
        authenticate_if_needed()
        until("wrong passphrase explained", lambda: any("passphrase" in e.get("text", "").lower()
              and "Enter this" not in e.get("text", "") for _, e in elements()))
        machine.screenshot("06-wrong-passphrase")
        machine.succeed("test ! -e " + clear)
        input_("type disposable-volume-pass")
        input_("key Return")
        authenticate_if_needed()
        machine.wait_for_text(r"encrypted\.txt", timeout=30)
        until("unlocked label in sidebar", lambda: find(label="QA_PRIVATE"))
        machine.screenshot("07-correct-passphrase-content")
        assert not find(name="unlock-pass")
        assert machine.succeed("findmnt -rn -o TARGET --source " + clear).strip() == point
        assert machine.succeed("cat " + point + "/encrypted.txt") == "private fixture content\n"
    with subtest("mounted volume accepts normal Hako file operations and relaunch"):
        input_("chord shift+super+n")
        input_("type Hako-created")
        input_("key Return")
        machine.wait_until_succeeds("test -d " + point + "/Hako-created")
        machine.screenshot("08-created-folder")
        quit_hako()
        launch_hako()
        click_label("QA_PRIVATE")
        machine.wait_for_text("Hako-created", timeout=30)
        machine.screenshot("09-fresh-hako-content")
        machine.fail("coredumpctl --no-pager list aizome-hako")
        quit_hako()
finally:
    machine.screenshot("final")
    Path(machine.out_dir, "inputs.json").write_text(json.dumps(inputs, indent=2))
    machine.succeed("journalctl _UID=1000 --no-pager > /tmp/encrypted-user.log")
    machine.succeed("journalctl -u udisks2 --no-pager > /tmp/encrypted-udisks.log")
    machine.succeed("udisksctl dump > /tmp/encrypted-udisks-dump.txt")
    machine.succeed("lsblk -J -o NAME,TYPE,LABEL,MOUNTPOINTS > /tmp/encrypted-lsblk.json")
    for path in ["/tmp/encrypted-user.log", "/tmp/encrypted-udisks.log", "/tmp/encrypted-udisks-dump.txt", "/tmp/encrypted-lsblk.json"]:
        machine.copy_from_machine(path)
    if machine.execute("test -f /tmp/encrypted-api.log")[0] == 0:
        machine.copy_from_machine("/tmp/encrypted-api.log")
    machine.copy_from_machine(INSPECT)
    machine.shutdown()
