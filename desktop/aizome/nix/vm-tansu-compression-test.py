"""UDisks policy used by Tansu; mutate only the VM's disposable /dev/vdb."""
import shlex


def user(command):
    return machine.succeed("su - alice -c " + shlex.quote(command))


def mounted():
    return machine.succeed("findmnt -nr -S /dev/vdb -o TARGET").strip()


machine.start()
# UDisks is D-Bus activated; this minimal VM has no desktop client to start it.
machine.succeed("systemctl start udisks2.service")
machine.wait_for_unit("udisks2.service")
machine.succeed("udevadm settle")
with subtest("Tansu's plain Format and Mount calls inherit zstd level 1"):
    user("busctl call org.freedesktop.UDisks2 /org/freedesktop/UDisks2/block_devices/vdb "
         "org.freedesktop.UDisks2.Block Format 'sa{sv}' btrfs 2 "
         "label s TANSU_COMPRESSION take-ownership b true")
    user("udisksctl mount -b /dev/vdb --no-user-interaction")
    options = machine.succeed("findmnt -nr -S /dev/vdb -o OPTIONS").strip().split(",")
    assert "compress=zstd:1" in options, options
    path = mounted() + "/compressible.txt"
    user("python3 -c " + shlex.quote(
        f"from pathlib import Path; Path({path!r}).write_bytes(b'tansu compression policy\\n' * 700000)"))
    machine.succeed("sync -f " + shlex.quote(path))
    report = machine.succeed("compsize -b " + shlex.quote(path))
    rows = [line.split() for line in report.splitlines()]
    zstd = next(row for row in rows if row and row[0] == "zstd")
    assert int(zstd[2]) < int(zstd[3]) // 2, report

with subtest("unmount and remount keep the same compression policy"):
    user("udisksctl unmount -b /dev/vdb --no-user-interaction")
    user("udisksctl mount -b /dev/vdb --no-user-interaction")
    options = machine.succeed("findmnt -nr -S /dev/vdb -o OPTIONS").strip().split(",")
    assert "compress=zstd:1" in options, options
    user("python3 -c " + shlex.quote(
        f"from pathlib import Path; assert Path({path!r}).read_bytes() == b'tansu compression policy\\n' * 700000"))

with subtest("other filesystem types still mount normally"):
    user("udisksctl unmount -b /dev/vdb --no-user-interaction")
    user("busctl call org.freedesktop.UDisks2 /org/freedesktop/UDisks2/block_devices/vdb "
         "org.freedesktop.UDisks2.Block Format 'sa{sv}' ext4 0")
    user("udisksctl mount -b /dev/vdb --no-user-interaction")
    assert machine.succeed("findmnt -nr -S /dev/vdb -o FSTYPE").strip() == "ext4"
machine.shutdown()
