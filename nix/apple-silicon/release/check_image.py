"""Check a Mac root image's actual bytes before it is packaged (image.nix).

- Its /nix-path-registration is byte for byte the closure's registration,
  and the paths it registers are exactly the closure (no more, no fewer).
- /nix/store in the image holds exactly those paths, and each one, read out
  of the ext4 image with debugfs, has the NAR hash and size it is registered
  with: the closure is present in the image's bytes, not only listed.
- /etc/nixos holds the shipped flake, lock and configuration, unchanged.
- The filesystem has the label and UUID the image declares.

Reads the image with e2fsprogs only; nothing is mounted.
"""

import argparse
import hashlib
import os
import shutil
import stat
import struct
import subprocess
import sys
import tempfile
from pathlib import Path


def debugfs(image: Path, request: str, binary=False):
    out = subprocess.run(["debugfs", "-R", request, str(image)], capture_output=True, check=True)
    return out.stdout if binary else out.stdout.decode()


def parse_registration(text: str) -> dict:
    """closureInfo's registration (nix-store --load-db): path, NAR hash, NAR
    size, deriver, reference count, references; one record after another."""
    lines = text.split("\n")
    records, i = {}, 0
    while i < len(lines) and lines[i]:
        path, nar_hash, nar_size = lines[i], lines[i + 1], int(lines[i + 2])
        count = int(lines[i + 4])
        records[path] = (nar_hash, nar_size)
        i += 5 + count
    return records


def store_listing(image: Path) -> set:
    names = set()
    for line in debugfs(image, "ls -p /nix/store").splitlines():
        # /inode/mode/uid/gid/name/size/
        fields = line.split("/")
        if len(fields) >= 6 and fields[5] not in (".", ".."):
            names.add("/nix/store/" + fields[5])
    return names


NIX32 = "0123456789abcdfghijklmnpqrsvwxyz"


def nix32(digest: bytes) -> str:
    """Nix's base-32 (registration hashes are written in it)."""
    out = []
    for i in range((len(digest) * 8 - 1) // 5, -1, -1):
        b, k = divmod(i * 5, 8)
        c = digest[b] >> k
        if b + 1 < len(digest):
            c |= digest[b + 1] << (8 - k)
        out.append(NIX32[c & 0x1F])
    return "".join(out)


class Nar:
    """The NAR serialization of a path, hashed and counted as it is
    produced (Nix's nix-archive-1), without needing a Nix store."""

    def __init__(self):
        self.h = hashlib.sha256()
        self.size = 0

    def raw(self, data: bytes):
        self.h.update(data)
        self.size += len(data)

    def str(self, s: bytes):
        self.raw(struct.pack("<Q", len(s)))
        self.raw(s)
        self.raw(b"\0" * (-len(s) % 8))

    def node(self, path: Path):
        self.str(b"(")
        st = os.lstat(path)
        if stat.S_ISLNK(st.st_mode):
            self.str(b"type")
            self.str(b"symlink")
            self.str(b"target")
            self.str(os.fsencode(os.readlink(path)))
        elif stat.S_ISREG(st.st_mode):
            self.str(b"type")
            self.str(b"regular")
            if st.st_mode & 0o100:
                self.str(b"executable")
                self.str(b"")
            self.str(b"contents")
            self.raw(struct.pack("<Q", st.st_size))
            with open(path, "rb") as f:
                while chunk := f.read(1 << 20):
                    self.raw(chunk)
            self.raw(b"\0" * (-st.st_size % 8))
        elif stat.S_ISDIR(st.st_mode):
            self.str(b"type")
            self.str(b"directory")
            for name in sorted(os.listdir(path), key=os.fsencode):
                self.str(b"entry")
                self.str(b"(")
                self.str(b"name")
                self.str(os.fsencode(name))
                self.str(b"node")
                self.node(path / name)
                self.str(b")")
        else:
            raise RuntimeError(f"{path} is neither a file, a directory nor a symlink")
        self.str(b")")


def nar(path: Path) -> tuple:
    n = Nar()
    n.str(b"nix-archive-1")
    n.node(path)
    return "sha256:" + nix32(n.h.digest()), n.size


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("--image", type=Path, required=True)
    p.add_argument("--closure-info", type=Path, required=True)
    p.add_argument("--etc-nixos", type=Path, required=True)
    p.add_argument("--label", required=True)
    p.add_argument("--uuid", required=True)
    p.add_argument("--report", type=Path, required=True)
    a = p.parse_args()
    problems, report = [], []

    registration = (a.closure_info / "registration").read_text()
    in_image = debugfs(a.image, "cat /nix-path-registration")
    if in_image != registration:
        problems.append("/nix-path-registration differs from the closure's registration")
    records = parse_registration(in_image)
    closure = set((a.closure_info / "store-paths").read_text().split())
    if set(records) != closure:
        problems.append(
            f"registered paths differ from the closure: {len(set(records) - closure)} extra, "
            f"{len(closure - set(records))} missing"
        )
    listing = store_listing(a.image)
    if listing != closure:
        problems.append(
            f"/nix/store differs from the closure: extra {sorted(listing - closure)[:5]}, "
            f"missing {sorted(closure - listing)[:5]}"
        )
    report.append(f"registered paths: {len(records)} (the closure: {len(closure)}; in /nix/store: {len(listing)})")

    total = 0
    with tempfile.TemporaryDirectory() as tmp:
        for n, path in enumerate(sorted(records)):
            out = Path(tmp) / "out"
            out.mkdir()
            debugfs(a.image, f"rdump {path} {out}")
            copy = out / Path(path).name
            if not copy.exists() and not copy.is_symlink():
                problems.append(f"{path} could not be read from the image")
            else:
                got = nar(copy)
                if got != records[path]:
                    problems.append(f"{path}: the image has {got}, registered {records[path]}")
                total += got[1]
            shutil.rmtree(out)
            if n % 500 == 0:
                print(f"checked {n}/{len(records)} paths", file=sys.stderr, flush=True)
    report.append(f"NAR hashes and sizes read from the image's bytes: {len(records)} paths, {total} bytes, all as registered")

    for f in sorted(a.etc_nixos.iterdir()):
        shipped = debugfs(a.image, f"cat /etc/nixos/{f.name}", binary=True)
        if shipped != f.read_bytes():
            problems.append(f"/etc/nixos/{f.name} differs from the shipped file")
    shipped_names = {l.split("/")[5] for l in debugfs(a.image, "ls -p /etc/nixos").splitlines() if l.count("/") >= 6}
    if shipped_names - {".", ".."} != {f.name for f in a.etc_nixos.iterdir()}:
        problems.append(f"/etc/nixos holds {sorted(shipped_names)}")
    report.append(f"/etc/nixos: {', '.join(sorted(f.name for f in a.etc_nixos.iterdir()))}, as shipped")

    header = subprocess.run(["dumpe2fs", "-h", str(a.image)], capture_output=True, text=True, check=True).stdout
    fields = dict(l.split(":", 1) for l in header.splitlines() if ":" in l)
    label, uuid = fields.get("Filesystem volume name", "").strip(), fields.get("Filesystem UUID", "").strip()
    if (label, uuid) != (a.label, a.uuid):
        problems.append(f"the filesystem is {label}/{uuid}, not {a.label}/{a.uuid}")
    report.append(f"filesystem: ext4, label {label}, UUID {uuid}, {a.image.stat().st_size} bytes")

    if problems:
        print("\n".join(problems), file=sys.stderr)
        return 1
    a.report.write_text("\n".join(report) + "\n")
    print("\n".join(report))
    return 0


if __name__ == "__main__":
    sys.exit(main())
