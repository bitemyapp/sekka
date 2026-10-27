"""Real pointer/keyboard grow/shrink regression, confined to VM /dev/vdb.

The fixture seeds the disk and test data; all mounts, unmounts and resizes use
Tansu's normal controls. Inspection only locates controls and reads visible text.
Backend assertions verify actual partition/filesystem sizes, UUID and file bytes.
"""
import datetime
import re
import json
import shlex
import time
from pathlib import Path

APP = "org.aizome.Tansu"
INSPECT = "/run/user/1000/resize-inspect"
inputs = []
assertions = []

def user(command):
    return machine.succeed("su - alice -c " + shlex.quote(
        "export XDG_RUNTIME_DIR=/run/user/1000; "
        "export WAYLAND_DISPLAY=$(ls /run/user/1000 | grep -m1 '^wayland-[0-9]*$'); "
        "export AIZOME_SOCKET=/run/user/1000/aizome.$WAYLAND_DISPLAY.sock; " + command))

def input_(command):
    inputs.append(command)
    user("aizomectl input " + command)
    time.sleep(0.2)

def window():
    return next((w for w in json.loads(user("aizomectl --json windows"))["windows"] if w["app_id"] == APP), None)

def documents():
    return [json.loads(s) for s in machine.succeed(
        f'for f in {INSPECT}/*.json; do if test -f "$f"; then cat "$f"; printf "\\n"; fi; done'
    ).splitlines() if s.strip()]

def elements():
    w = window()
    return [e for d in documents() if w and d.get("app_id") == APP and d.get("pid") == w["pid"]
            and d.get("kind") == "window" for e in d["elements"] if e["visible"]]

def find(**attributes):
    return next((e for e in elements() if all(e.get(k) == v for k, v in attributes.items())), None)

def until(description, predicate):
    deadline = time.monotonic() + 25
    while time.monotonic() < deadline:
        value = predicate()
        if value:
            return value
        time.sleep(0.2)
    raise AssertionError(description)

def click(**attributes):
    e = until(str(attributes), lambda: find(**attributes))
    x, y, width, height = e["rect"]
    g = window()["geometry"]
    input_(f"motion {g['x'] + x + width / 2} {g['y'] + y + height / 2}")
    input_("click")

def authenticate_if_needed():
    for _ in range(20):
        if any(d.get("title") == "Authenticate" and any(e.get("name") == "auth-password" and e["visible"] for e in d["elements"]) for d in documents()):
            input_("type test-only")
            input_("key Return")
            return
        time.sleep(0.2)

def mount_point():
    return machine.succeed("findmnt -rn -o TARGET --source /dev/vdb1 || true").strip()


def disk_state(label):
    info = machine.succeed("dumpe2fs -h /dev/vdb1 2>/dev/null")
    fields = dict(re.findall(r"^([^:\n]+):\s*(.*)$", info, re.MULTILINE))
    state = {
        "partition_bytes": int(machine.succeed("blockdev --getsize64 /dev/vdb1")),
        "filesystem_bytes": int(fields["Block count"]) * int(fields["Block size"]),
        "uuid": fields["Filesystem UUID"],
        "start_sectors": int(machine.succeed("cat /sys/class/block/vdb1/start")),
        "last_mount": fields["Last mount time"],
        "last_check": fields["Last checked"],
    }
    assertions.append({"label": label, **state})
    Path(machine.out_dir, label + "-superblock.txt").write_text(info)
    return state


def hashes():
    point = mount_point()
    assert point.startswith("/run/media/alice/"), point
    result = machine.succeed("cd " + shlex.quote(point) + "; sha256sum preservation/*.bin")
    assert len(result.splitlines()) == 10, result
    return result


def mounted():
    click(text="Mount", role="button")
    authenticate_if_needed()
    return until("filesystem mounted through Tansu", mount_point)


def unmounted():
    click(text="Unmount", role="button")
    until("filesystem unmounted through Tansu", lambda: not mount_point())


def resize(text, label):
    click(text="Resize", role="button")
    click(name="resize-size")
    input_("chord super+a")
    input_("type " + shlex.quote(text))
    machine.screenshot(label + "-size")
    click(text="Grow" if label.endswith("grow") else "Shrink", role="button")
    click(text="Apply…", role="button")
    until("apply confirmation", lambda: find(name="type-0"))
    if not expect_failure:
        assert any("checked and repaired" in e.get("text", "") for e in elements())
    click(name="type-0")
    input_("type vdb")
    machine.screenshot(label + "-confirm")
    click(text="Apply 1 Operation", role="button")
    authenticate_if_needed()
    until("resize finished", lambda: find(text="Done", role="button"))
    texts = [e.get("text", "") for e in elements()]
    machine.screenshot(label + "-result")
    assertions.append({"label": label + "-visible-result", "texts": texts})
    if expect_failure:
        assert any("e2fsck -f" in text for text in texts), texts
        assert any("It stopped at the operation that failed" in text for text in texts), texts
    else:
        assert "Every operation ran." in texts, texts
    click(text="Done", role="button")


machine.start()
machine.wait_until_succeeds("pgrep -u alice -x aizome-shell", timeout=120)
try:
    # The test runner creates this marker and this empty 2GiB secondary disk.
    machine.succeed("test \"$(cat /etc/aizome-tansu-resize-test)\" = disposable-resize-test")
    machine.succeed("test \"$(blockdev --getsize64 /dev/vdb)\" = 2147483648; test -z \"$(findmnt -rn --source /dev/vdb)\"")
    machine.succeed("printf 'label: dos\nstart=2048,size=1572864,type=83\n' | sfdisk /dev/vdb")
    machine.wait_until_succeeds("test -b /dev/vdb1")
    machine.succeed("mkfs.ext4 -F -b 4096 -E root_owner=1000:100 -L TANSU_GROW /dev/vdb1")
    initial = disk_state("initial")
    assert initial["partition_bytes"] == 768 * 1024 * 1024
    assert initial["start_sectors"] == 2048
    # ext4 timestamps have one-second precision: the later normal mount must
    # demonstrably occur after mkfs's last check for the negative control.
    time.sleep(2)
    user(f"aizomectl spawn env AIZOME_KIT_INSPECT={INSPECT} aizome-tansu --show=/dev/vdb")
    until("Tansu window", window)
    until("test partition", lambda: find(text="vdb1"))
    click(text="vdb1")
    point = mounted()
    user("mkdir -p " + shlex.quote(point + "/preservation") + "; for n in $(seq 1 10); do head -c 1048576 /dev/urandom > " + shlex.quote(point + "/preservation/file-") + '"$n".bin; done; sync')
    before_hashes = hashes()
    Path(machine.out_dir, "files-before.txt").write_text(before_hashes)
    machine.screenshot("01-mounted-files")
    unmounted()
    before = disk_state("before-grow")
    date_format = "%a %b %d %H:%M:%S %Y"
    assert datetime.datetime.strptime(before["last_check"], date_format) < datetime.datetime.strptime(before["last_mount"], date_format)
    resize("1.6 GB", "02-grow")
    grown = disk_state("after-grow")
    expected = 1_600_000_000 // (1 << 20) * (1 << 20)
    assert abs(grown["partition_bytes"] - expected) <= 1 << 20
    assert grown["uuid"] == initial["uuid"] and grown["start_sectors"] == 2048
    if expect_failure:
        assert grown["filesystem_bytes"] == initial["filesystem_bytes"], "the failed resize must not be claimed as success"
        click(text="Clear", role="button")
        machine.screenshot("03-failed-plan-cleared")
    else:
        assert abs(grown["filesystem_bytes"] - grown["partition_bytes"]) <= 4096
    mounted()
    after_grow_hashes = hashes()
    assert after_grow_hashes == before_hashes
    Path(machine.out_dir, "files-after-grow.txt").write_text(after_grow_hashes)
    machine.screenshot("04-grown-files-preserved")
    unmounted()
    if not expect_failure:
        resize("1.0 GB", "05-shrink")
        shrunk = disk_state("after-shrink")
        expected = 1_000_000_000 // (1 << 20) * (1 << 20)
        assert abs(shrunk["partition_bytes"] - expected) <= 1 << 20
        assert abs(shrunk["filesystem_bytes"] - shrunk["partition_bytes"]) <= 4096
        assert shrunk["uuid"] == initial["uuid"] and shrunk["start_sectors"] == 2048
        mounted()
        assert hashes() == before_hashes
        Path(machine.out_dir, "files-after.txt").write_text(hashes())
        machine.screenshot("06-shrunk-files-preserved")
        unmounted()
    machine.succeed("test -z \"$(systemctl --failed --no-legend)\"")
    machine.fail("coredumpctl --no-pager list aizome-tansu")
finally:
    machine.screenshot("final")
    Path(machine.out_dir, "inputs.json").write_text(json.dumps(inputs, indent=2))
    Path(machine.out_dir, "assertions.json").write_text(json.dumps(assertions, indent=2))
    machine.succeed("journalctl -b --no-pager > /tmp/tansu-resize-journal.log")
    machine.copy_from_machine("/tmp/tansu-resize-journal.log")
    machine.succeed(f"tar -C {INSPECT} -cf /tmp/resize-inspect.tar .")
    machine.copy_from_machine("/tmp/resize-inspect.tar")
    machine.shutdown()
