"""Replace m1n1's second stage (m1n1/boot.bin) on the EFI system partition,
verified, keeping the copy it replaces.

The Asahi stub's m1n1 loads m1n1/boot.bin from this installation's ESP before
anything else of Sekka's runs: m1n1, every Apple device tree, U-Boot and
m1n1's settings, in one file. All generations share it. NixOS's own copy
(systemd-boot's extraFiles) deletes the file and then copies the new one in
place, so an interruption can leave the Mac with no second stage, and the
replaced copy is gone. Instead, on every boot loader installation
(module.nix, extraInstallCommands):

1. Leftovers of an interrupted run (*.new) are removed; the active copy was
   never touched by one, or was already completely replaced.
2. Nothing changes when the active copy already has the new digest.
3. The active copy is copied to m1n1/boot.bin.previous.new, flushed, read back
   and checked, then renamed over m1n1/boot.bin.previous.
4. sekka/boot-bin.json records the replacement as pending.
5. The new file is written as m1n1/boot.bin.new, flushed, read back from the
   disk and checked, then renamed over m1n1/boot.bin, and the directory is
   flushed. The active copy is read back and checked once more.
6. sekka/boot-bin.json records the new current and previous copies and a short
   history.

FAT has no journal: a rename is a single directory-entry update, the
smallest change it can make, not a transaction. If one is interrupted,
m1n1/boot.bin.previous and sekka/boot-bin.json say exactly which bytes to
put back, from Linux (`restore-previous`) or from macOS.

  boot_bin.py install --esp /boot --source BOOT.BIN [--system TOPLEVEL]
  boot_bin.py restore-previous --esp /boot
  boot_bin.py status --esp /boot
"""

import argparse
import datetime
import hashlib
import json
import os
import subprocess
import sys
from pathlib import Path

SCHEMA = "sekka.mac.boot-bin/1"
ACTIVE = "m1n1/boot.bin"
PREVIOUS = "m1n1/boot.bin.previous"
RECORD = "sekka/boot-bin.json"
HISTORY = 10
# Room left on the ESP beyond the files being written.
MARGIN = 4 * 1024 * 1024


class Failed(Exception):
    """The replacement stopped; the active copy is as the message says."""


def crash_point(name: str) -> None:
    """Tests stop the procedure here to model an interruption."""


def digest(data: bytes) -> str:
    return "sha256:" + hashlib.sha256(data).hexdigest()


def now() -> str:
    return datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def canonical(value) -> bytes:
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()


def fsync_dir(path: Path) -> None:
    fd = os.open(path, os.O_RDONLY | os.O_DIRECTORY)
    try:
        os.fsync(fd)
    finally:
        os.close(fd)


def read_back(path: Path) -> bytes:
    """Read a file from the disk rather than from the page cache: its pages
    are clean after fsync, so dropping them makes the read go to the disk."""
    with open(path, "rb") as f:
        os.posix_fadvise(f.fileno(), 0, 0, os.POSIX_FADV_DONTNEED)
        return f.read()


def write_verified(path: Path, data: bytes) -> None:
    with open(path, "wb") as f:
        f.write(data)
        f.flush()
        os.fsync(f.fileno())
    back = read_back(path)
    if back != data:
        path.unlink(missing_ok=True)
        raise Failed(f"{path} read back as {digest(back)}, not the {digest(data)} written")


def replace(tmp: Path, path: Path) -> None:
    os.replace(tmp, path)
    fsync_dir(path.parent)


def entry(data: bytes, source=None, system=None, installed=None) -> dict:
    return {
        "digest": digest(data),
        "size": len(data),
        "source": source,
        "system": system,
        "installed": installed,
    }


def empty_record() -> dict:
    return {"schema": SCHEMA, "current": None, "previous": None, "pending": None, "history": []}


def load_record(esp: Path, warn) -> dict:
    path = esp / RECORD
    if not path.exists():
        return empty_record()
    try:
        value = json.loads(path.read_bytes())
        if value.get("schema") != SCHEMA or not isinstance(value.get("history"), list):
            raise ValueError(f"schema {value.get('schema')!r}")
        for key in ("current", "previous", "pending"):
            item = value.get(key)
            if item is not None and not (isinstance(item, dict) and str(item.get("digest", "")).startswith("sha256:")):
                raise ValueError(f"{key} is {item!r}")
        return value
    except ValueError as e:
        warn(f"{path} is unreadable ({e}); it is rebuilt from the files on the ESP")
        return empty_record()


def save_record(esp: Path, record: dict) -> None:
    path = esp / RECORD
    path.parent.mkdir(exist_ok=True)
    tmp = path.with_name(path.name + ".new")
    with open(tmp, "wb") as f:
        f.write(canonical(record))
        f.flush()
        os.fsync(f.fileno())
    replace(tmp, path)


def clean_leftovers(esp: Path, say) -> None:
    for name in (ACTIVE + ".new", PREVIOUS + ".new", RECORD + ".new"):
        path = esp / name
        if path.exists():
            say(f"removing {name}, left by an interrupted replacement")
            path.unlink()
    for name in (ACTIVE, PREVIOUS, RECORD):
        if (esp / name).parent.exists():
            fsync_dir((esp / name).parent)


def recover_previous(candidates) -> tuple[bytes, str] | None:
    """The boot.bin a system installed with NixOS's extraFiles, found in its
    closure: NixOS deleted it from the ESP before this script ran."""
    for system in candidates:
        if not system or not Path(system).exists():
            continue
        try:
            closure = subprocess.run(
                ["nix-store", "--query", "--requisites", system], capture_output=True, text=True, check=True
            ).stdout.split()
        except (OSError, subprocess.CalledProcessError):
            continue
        found = [p for p in closure if p.endswith("-boot.bin") and Path(p).is_file()]
        if len(found) == 1:
            return Path(found[0]).read_bytes(), found[0]
    return None


def install(esp: Path, new: bytes, source=None, system=None, recover_from=(), say=print, warn=None) -> str:
    warn = warn or (lambda m: print(f"warning: {m}", file=sys.stderr))
    record = load_record(esp, warn)
    clean_leftovers(esp, say)
    (esp / "m1n1").mkdir(exist_ok=True)
    active_path = esp / ACTIVE
    active = active_path.read_bytes() if active_path.exists() else None
    new_digest = digest(new)

    if active is not None and digest(active) == new_digest:
        # An interrupted run may have renamed it in but not recorded it.
        current = record.get("current") or {}
        if current.get("digest") != new_digest or record.get("pending"):
            record["current"] = entry(new, source, system, now())
            record["pending"] = None
            if not record["history"] or record["history"][-1].get("digest") != new_digest:
                record["history"] = (record["history"] + [record["current"]])[-HISTORY:]
            save_record(esp, record)
            return f"{ACTIVE} is already {new_digest}; recorded it"
        return f"{ACTIVE} is unchanged ({new_digest})"

    stat = os.statvfs(esp)
    needed = len(new) + (len(active) if active is not None else 0) + MARGIN
    if stat.f_bavail * stat.f_frsize < needed:
        raise Failed(
            f"the ESP has {stat.f_bavail * stat.f_frsize} bytes free; replacing {ACTIVE} needs {needed}. "
            f"{ACTIVE} was not changed."
        )

    # The copy being replaced, kept and verified before anything else changes.
    old_entry = None
    if active is not None:
        old_entry = record.get("current") if (record.get("current") or {}).get("digest") == digest(active) else None
        old_entry = old_entry or entry(active)
        tmp = esp / (PREVIOUS + ".new")
        write_verified(tmp, active)
        crash_point("previous-written")
        replace(tmp, esp / PREVIOUS)
        crash_point("previous-replaced")
    else:
        recovered = recover_previous(recover_from)
        if recovered is not None:
            data, path = recovered
            if digest(data) != new_digest:
                say(f"{ACTIVE} was missing; keeping {path}, the copy the running system installed, as {PREVIOUS}")
                tmp = esp / (PREVIOUS + ".new")
                write_verified(tmp, data)
                replace(tmp, esp / PREVIOUS)
                old_entry = entry(data, source=path)
        else:
            warn(f"{ACTIVE} was missing; there is no copy to keep")

    record["previous"] = old_entry if old_entry is not None else record.get("previous")
    record["pending"] = entry(new, source, system)
    save_record(esp, record)
    crash_point("pending-recorded")

    tmp = esp / (ACTIVE + ".new")
    write_verified(tmp, new)
    crash_point("new-written")
    replace(tmp, active_path)
    crash_point("new-replaced")
    back = read_back(active_path)
    if digest(back) != new_digest:
        raise Failed(
            f"{ACTIVE} read back as {digest(back)} after replacing it with {new_digest}; "
            f"{PREVIOUS} holds {record['previous']['digest'] if record['previous'] else 'nothing'}"
        )

    record["current"] = entry(new, source, system, now())
    record["pending"] = None
    record["history"] = (record["history"] + [record["current"]])[-HISTORY:]
    save_record(esp, record)
    return f"{ACTIVE} is now {new_digest}" + (
        f"; {PREVIOUS} keeps {record['previous']['digest']}" if record["previous"] else ""
    )


def restore_previous(esp: Path, say=print, warn=None) -> str:
    path = esp / PREVIOUS
    if not path.exists():
        raise Failed(f"there is no {PREVIOUS} to restore")
    data = read_back(path)
    record = load_record(esp, warn or (lambda m: print(f"warning: {m}", file=sys.stderr)))
    expected = (record.get("previous") or {}).get("digest")
    if expected and expected != digest(data):
        raise Failed(f"{PREVIOUS} is {digest(data)}, but {RECORD} says it should be {expected}; nothing was changed")
    source = (record.get("previous") or {}).get("source")
    system = (record.get("previous") or {}).get("system")
    return install(esp, data, source=source, system=system, say=say, warn=warn)


def status(esp: Path) -> dict:
    record = load_record(esp, lambda m: None)
    files = {}
    for name in (ACTIVE, PREVIOUS):
        p = esp / name
        files[name] = digest(p.read_bytes()) if p.exists() else None
    consistent = files[ACTIVE] == (record.get("current") or {}).get("digest") and (
        record.get("previous") is None or files[PREVIOUS] == record["previous"]["digest"]
    )
    return {"files": files, "record": record, "consistent": consistent and not record.get("pending")}


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    sub = parser.add_subparsers(dest="command", required=True)
    i = sub.add_parser("install")
    i.add_argument("--esp", type=Path, required=True)
    i.add_argument("--source", type=Path, required=True)
    i.add_argument("--system")
    i.add_argument("--recover-from", action="append", default=[])
    r = sub.add_parser("restore-previous")
    r.add_argument("--esp", type=Path, required=True)
    s = sub.add_parser("status")
    s.add_argument("--esp", type=Path, required=True)
    args = parser.parse_args(argv)
    say = lambda m: print(f"sekka: {m}")  # noqa: E731
    try:
        if args.command == "install":
            source = args.source.resolve()
            say(install(args.esp, source.read_bytes(), str(source), args.system, args.recover_from, say))
        elif args.command == "restore-previous":
            say(restore_previous(args.esp, say))
        else:
            print(json.dumps(status(args.esp), indent=2, sort_keys=True))
    except Failed as e:
        print(f"sekka: {e}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
