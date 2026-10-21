"""Run the generated unit script with only files present in the real initrd.

The component VM has a complete Nix store and cannot catch missing initrd shell
dependencies. PRoot confines path resolution to the unpacked archive, including
the dynamic loader and libraries, without requiring privileged chroot/mounts.
"""
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import stat
import subprocess
import sys

initrd, helper, output = sys.argv[1:]
blob = Path(initrd).read_bytes()
remaining = blob
entries = {}
while remaining:
    remaining = remaining.lstrip(b"\0")
    if not remaining:
        break
    if remaining.startswith(bytes.fromhex("28b52ffd")):
        remaining = subprocess.run(
            ["zstd", "-d", "-q", "-c"], input=remaining, check=True,
            stdout=subprocess.PIPE,
        ).stdout
    # A microcode cpio may precede the compressed main archive.
    position = 0
    while True:
        header = remaining[position:position + 110]
        assert header[:6] in (b"070701", b"070702"), header[:6]
        mode = int(header[14:22], 16)
        size = int(header[54:62], 16)
        name_size = int(header[94:102], 16)
        name = remaining[position + 110:position + 110 + name_size - 1].decode()
        start = (position + 110 + name_size + 3) // 4 * 4
        payload = remaining[start:start + size]
        assert len(payload) == size
        position = (start + size + 3) // 4 * 4
        if name == "TRAILER!!!":
            break
        assert not name.startswith("/") and ".." not in Path(name).parts, name
        entries[name.removeprefix("./")] = (mode, payload)
    remaining = remaining[position:]

assert helper.lstrip("/") in entries, f"helper missing from initrd: {helper}"
units = [payload.decode() for name, (mode, payload) in entries.items()
         if name.endswith("/copytoram.service") and stat.S_ISREG(mode)]
assert len(units) == 1, units
unit = units[0]
assert "ConditionKernelCommandLine=copytoram" in unit, unit
job = re.search(r"^ExecStart=(\S+)", unit, re.M).group(1)
job_text = entries[job.lstrip("/")][1].decode()
assert f"exec {helper}\n" in job_text, job_text
script = entries[helper.lstrip("/")][1].decode()
shell = script.splitlines()[0].removeprefix("#!")
assert shell.lstrip("/") in entries, f"interpreter missing: {shell}"
runtime_path = re.search(r'^export PATH="([^"]+)"', script, re.M).group(1)
runtime_path = runtime_path.removesuffix(":$PATH")
assert "$" not in runtime_path, runtime_path

# Materialize files before absolute symlinks, so extraction never follows a
# store symlink into the builder's host filesystem.
root = Path("root").resolve()
root.mkdir()
for name, (mode, payload) in entries.items():
    path = root / name
    if stat.S_ISDIR(mode):
        path.mkdir(parents=True, exist_ok=True)
    elif stat.S_ISREG(mode):
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(payload)
        path.chmod(stat.S_IMODE(mode))
for name, (mode, payload) in entries.items():
    if stat.S_ISLNK(mode):
        path = root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.symlink_to(payload.decode())
for name in ["dev/console", "dev/null"]:
    path = root / name
    path.parent.mkdir(parents=True, exist_ok=True)
    path.touch()

# Execute every external command used by the helper, with no host PATH/store
# fallback. This also exercises their packaged ELF interpreters/libraries.
commands = ["findmnt", "blockdev", "stat", "mktemp", "mkdir", "mountpoint",
            "umount", "rmdir", "mount", "cp"]
environment = dict(os.environ, PATH=runtime_path)
proot = shutil.which("proot")
assert proot


def run(arguments):
    return subprocess.run(
        [proot, "-r", str(root), "-w", "/", *arguments], env=environment,
        text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, check=True,
    ).stdout


for command in commands:
    run([shell, "-c", 'exec "$1" --version', "check", command])
# No media/proc mounts are supplied: exercise the actual service entrypoint's
# safe unavailable-media fallback. Copy semantics are covered by the VM test.
result = run([job])
assert "cannot determine media size; continuing from installation media." in result, result
Path(output).write_text(json.dumps({
    "initrd_sha256": hashlib.sha256(blob).hexdigest(),
    "helper": helper, "unit_entrypoint": job, "runtime_commands": commands,
    "unavailable_media_fallback": result,
}) + "\n")
