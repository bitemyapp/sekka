"""Replace m1n1's second stage (m1n1/boot.bin) on the EFI system partition so
that the ESP names one complete copy at every instant, and keep a copy that
booted.

The Asahi stub's m1n1 loads m1n1/boot.bin from this installation's ESP before
anything else of Sekka's runs: m1n1, every Apple device tree, U-Boot and
m1n1's settings, in one file. All generations share it. NixOS's own copy
(systemd-boot's extraFiles) deletes the file and then copies the new one in
place. This script runs instead on every boot loader installation
(module.nix, extraInstallCommands).

What FAT leaves after a power cut. A FAT file is a directory entry (name,
first cluster, size), a chain in the FAT and its data clusters. Linux's vfat
(fs/fat) changes all of them in memory first, and writes them back in no
order it promises:
- rename(2) over an existing name gives the renamed file the target's
  entry, but writes the new first cluster and size into it only when the
  file's inode is written back; it marks the source entry deleted in the
  directory's buffer; and the replaced file's clusters are freed in the FAT
  when its inode is evicted, which is at once unless something holds it
  open;
- fsync(2) of a directory writes the directory's own changed sectors (new
  and deleted entries) and the FAT; fsync of a file writes its data, the
  FAT, and its entry only if its inode is still dirty: background writeback
  may already have copied the inode into a directory sector it has not
  written;
- syncfs(2) writes every inode, every dirty sector and the FAT.

So replacing a name (`commit`) is: hold the replaced file open (its clusters
stay allocated), rename, fsync the directory (the source entry's removal
reaches the disk first), syncfs (the target entry now names the new
clusters, however its inode was written), then close the replaced file (only
now are its clusters freed). At every instant the disk's entry names either
the old clusters, still allocated and intact, or the new ones, written and
flushed before the rename. One state remains possible: a write-back of the
renamed inode during the rename, or in the microseconds before the
directory's fsync, then a power cut, leaves the target and its staged copy
naming the same new clusters. The target is complete in it; deleting or
truncating either would free the other's clusters, so a leftover that shares
its first block with the file it was renamed over (FIBMAP) is refused, not
deleted. test_boot_bin.py runs every operation against a model of these
semantics and checks every state a power cut can leave.

On every boot loader installation:
1. An interrupted replacement is finished from its staged copies, or, when
   one is gone, given up with the files as they are; leftovers (*.new)
   are removed.
2. Nothing changes when the active copy already has the new digest, or when
   it is the copy `restore-previous` (or a copy from macOS) put back over the
   one this installation would install again (the hold, below).
3. The copy being replaced becomes m1n1/boot.bin.previous only if it has
   booted (sekka-boot-bin.service records that at every start) or nothing
   else is kept: two installations without a restart keep the copy that
   booted.
4. The new previous copy and the new copy are written as *.new, flushed,
   read back from the disk and checked; sekka/boot-bin.json records the
   replacement as pending; boot.bin.previous is committed first, then
   boot.bin, each as above; the record then names the new current and
   previous copies.

  boot_bin.py install --esp /boot --source BOOT.BIN [--system TOPLEVEL]
  boot_bin.py restore-previous --esp /boot   swap the two copies, and hold
  boot_bin.py release --esp /boot            let installations replace a held copy
  boot_bin.py mark-booted --esp /boot        at every start: this copy booted
  boot_bin.py status --esp /boot
  boot_bin.py forget-extra-files --esp /boot before NixOS's systemd-boot
      installer: stop it deleting boot.bin, which a generation from before
      this script installed with extraFiles
"""

import argparse
import ctypes
import datetime
import fcntl
import hashlib
import json
import os
import struct
import subprocess
import sys
from pathlib import Path

SCHEMA = "sekka.mac.boot-bin/1"
ACTIVE = "m1n1/boot.bin"
PREVIOUS = "m1n1/boot.bin.previous"
RECORD = "sekka/boot-bin.json"
# What NixOS's systemd-boot builder deletes before copying extraFiles back
# (systemd-boot-builder.py, remove_extra_files).
EXTRA_FILES_MARKER = "EFI/nixos/.extra-files/m1n1/boot.bin"
HISTORY = 10
# Room left on the ESP beyond the files being written.
MARGIN = 4 * 1024 * 1024
FIBMAP = 1
MSDOS_SUPER_MAGIC = 0x4D44


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


def parent(rel: str) -> str:
    return rel.rsplit("/", 1)[0] if "/" in rel else ""


class Disk:
    """The ESP through Linux's system calls. Paths are relative to it."""

    def __init__(self, root):
        self.root = Path(root)
        self.libc = ctypes.CDLL(None, use_errno=True)

    def path(self, rel: str) -> Path:
        return self.root / rel if rel else self.root

    def exists(self, rel: str) -> bool:
        return self.path(rel).exists()

    def read(self, rel: str) -> bytes:
        """From the disk rather than the page cache: the pages are clean after
        fsync, so dropping them makes the read go to the disk."""
        with open(self.path(rel), "rb") as f:
            os.posix_fadvise(f.fileno(), 0, 0, os.POSIX_FADV_DONTNEED)
            return f.read()

    def write(self, rel: str, data: bytes) -> None:
        """A new file, flushed: its data and its clusters in the FAT."""
        fd = os.open(self.path(rel), os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        try:
            view = memoryview(data)
            while view:
                view = view[os.write(fd, view) :]
            os.fsync(fd)
        finally:
            os.close(fd)

    def hold(self, rel: str):
        return os.open(self.path(rel), os.O_RDONLY)

    def release(self, handle) -> None:
        os.close(handle)

    def rename(self, src: str, dst: str) -> None:
        os.rename(self.path(src), self.path(dst))

    def unlink(self, rel: str) -> None:
        self.path(rel).unlink()

    def mkdir(self, rel: str) -> None:
        if not self.path(rel).is_dir():
            self.path(rel).mkdir()
            self.sync_dir(parent(rel))

    def rmdir_if_empty(self, rel: str) -> None:
        try:
            self.path(rel).rmdir()
        except OSError:
            return
        self.sync_dir(parent(rel))

    def sync_dir(self, rel: str) -> None:
        fd = os.open(self.path(rel), os.O_RDONLY | os.O_DIRECTORY)
        try:
            os.fsync(fd)
        finally:
            os.close(fd)

    def sync_all(self) -> None:
        fd = os.open(self.root, os.O_RDONLY | os.O_DIRECTORY)
        try:
            if self.libc.syncfs(fd) != 0:
                e = ctypes.get_errno()
                raise OSError(e, f"syncfs {self.root}: {os.strerror(e)}")
        finally:
            os.close(fd)

    def free_bytes(self) -> int:
        stat = os.statvfs(self.root)
        return stat.f_bavail * stat.f_frsize

    def first_block(self, rel: str):
        """The disk block of the file's first byte (FIBMAP; root only), or None."""
        try:
            fd = os.open(self.path(rel), os.O_RDONLY)
        except OSError:
            return None
        try:
            return struct.unpack("i", fcntl.ioctl(fd, FIBMAP, struct.pack("i", 0)))[0]
        except OSError:
            return None
        finally:
            os.close(fd)

    def is_fat(self) -> bool:
        buf = ctypes.create_string_buffer(256)
        if self.libc.statfs(str(self.root).encode(), buf) != 0:
            return True  # unknown: assume the worst
        return struct.unpack_from("l", buf.raw)[0] == MSDOS_SUPER_MAGIC


def refuse_shared(disk, tmp: str) -> None:
    """Stop when a staged copy shares its clusters with the file it was
    renamed over (a write-back during the rename, then a power cut; the
    module's docstring): deleting or truncating either frees the other's."""
    target = tmp[: -len(".new")]
    if not (disk.exists(tmp) and disk.exists(target)):
        return
    a, b = disk.first_block(tmp), disk.first_block(target)
    if a is not None and b is not None:
        shared = a != 0 and a == b
    else:
        # Without FIBMAP: shared clusters hold the same bytes, so different
        # bytes are proof enough. On FAT, the same bytes are not.
        shared = disk.is_fat() and disk.read(tmp) == disk.read(target)
    if shared:
        raise Failed(
            f"{tmp} and {target} share their clusters on the disk (a power cut as {tmp} was renamed "
            f"over it). {target} is complete; deleting either would free the other's clusters, so "
            f"nothing was changed. fsck.vfat, which systemd runs on the ESP at every start, truncates "
            f"one of the two: copy {target} somewhere safe first, then restart or run fsck.vfat on the "
            f"unmounted ESP, and install again."
        )


def discard(disk, tmp: str, say=None) -> None:
    """Remove a staged copy (refuse_shared first)."""
    if not disk.exists(tmp):
        return
    refuse_shared(disk, tmp)
    if say:
        say(f"removing {tmp}, left by an interrupted replacement")
    disk.unlink(tmp)
    disk.sync_dir(parent(tmp))


def put(disk, rel: str, data: bytes) -> str:
    """Stage `data` as rel.new: written, flushed and read back from the disk."""
    tmp = rel + ".new"
    discard(disk, tmp)
    disk.write(tmp, data)
    # Its entry, long name included, as the directory's own sectors.
    disk.sync_dir(parent(tmp))
    back = disk.read(tmp)
    if back != data:
        disk.unlink(tmp)
        disk.sync_dir(parent(tmp))
        raise Failed(f"{tmp} read back as {digest(back)}, not the {digest(data)} written")
    return tmp


def commit(disk, tmp: str, rel: str) -> None:
    """Rename tmp over rel in the order FAT needs (the module's docstring):
    the disk names rel's old clusters or tmp's, never freed ones."""
    held = disk.hold(rel) if disk.exists(rel) else None
    try:
        disk.rename(tmp, rel)
        disk.sync_dir(parent(rel))
        disk.sync_all()
    finally:
        if held is not None:
            disk.release(held)


def entry(data: bytes, source=None, system=None, installed=None, booted=None) -> dict:
    return {
        "digest": digest(data),
        "size": len(data),
        "source": source,
        "system": system,
        "installed": installed,
        # True once a start found it at m1n1/boot.bin (mark-booted), False
        # until then; None when nobody knows (a record rebuilt from the files).
        "booted": booted,
    }


def empty_record() -> dict:
    return {"schema": SCHEMA, "current": None, "previous": None, "pending": None, "held": None, "history": []}


def valid_entry(item) -> bool:
    if not isinstance(item, dict):
        return False
    d = item.get("digest")
    if not (isinstance(d, str) and d.startswith("sha256:") and len(d) == 71):
        return False
    if not isinstance(item.get("size"), int) or isinstance(item.get("size"), bool) or item["size"] < 0:
        return False
    return all(item.get(k) is None or isinstance(item.get(k), str) for k in ("source", "system", "installed")) and (
        item.get("booted") in (None, True, False)
    )


def load_record(disk, warn) -> dict:
    """sekka/boot-bin.json, or an empty record when it is missing or anything
    in it is not what this script writes; the files on the ESP then say what
    is there."""
    if not disk.exists(RECORD):
        return empty_record()
    try:
        value = json.loads(disk.read(RECORD))
        if not isinstance(value, dict) or value.get("schema") != SCHEMA:
            raise ValueError(f"schema {value.get('schema') if isinstance(value, dict) else value!r}")
        for key in ("current", "previous"):
            if value.get(key) is not None and not valid_entry(value[key]):
                raise ValueError(f"{key} is {value[key]!r}")
        history = value.get("history")
        if not isinstance(history, list) or not all(valid_entry(h) for h in history):
            raise ValueError("history isn't a list of entries")
        pending = value.get("pending")
        if pending is not None and valid_entry(pending):
            # An older record's pending entry: an installation's new copy.
            pending = {"kind": "install", "active": pending, "previous": value.get("previous")}
        if pending is not None and not (
            isinstance(pending, dict)
            and pending.get("kind") in ("install", "restore")
            and valid_entry(pending.get("active"))
            and (pending.get("previous") is None or valid_entry(pending["previous"]))
        ):
            raise ValueError(f"pending is {pending!r}")
        held = value.get("held")
        if held is not None and not (
            isinstance(held, dict)
            and all(isinstance(held.get(k), str) for k in ("kept", "replaced", "how", "since"))
        ):
            raise ValueError(f"held is {held!r}")
        record = empty_record()
        record.update(
            current=value.get("current"),
            previous=value.get("previous"),
            pending=pending,
            held=held,
            history=history,
        )
        return record
    except (ValueError, TypeError, AttributeError, KeyError) as e:
        warn(f"{RECORD} is unreadable ({e}); it is rebuilt from the files on the ESP")
        return empty_record()


def save_record(disk, record: dict) -> None:
    disk.mkdir(parent(RECORD))
    commit(disk, put(disk, RECORD, canonical(record)), RECORD)


def file_digest(disk, rel: str):
    return digest(disk.read(rel)) if disk.exists(rel) else None


def known(record: dict, d: str, data: bytes) -> dict:
    """What the record knows of the copy with digest d, else a fresh entry."""
    p = record.get("pending") or {}
    for e in (record.get("current"), p.get("active"), p.get("previous"), record.get("previous")):
        if e is not None and e["digest"] == d:
            return dict(e)
    return entry(data)


def finish(disk, record: dict) -> bool:
    """Complete the pending replacement from its staged copies, in its
    order. False when a staged copy it still needs is missing or wrong."""
    p = record["pending"]
    order = [(ACTIVE, p["active"]), (PREVIOUS, p["previous"])]
    if p["kind"] == "install":
        # The copy being replaced is kept before anything replaces it.
        order.reverse()
    for rel, want in order:
        if want is None or file_digest(disk, rel) == want["digest"]:
            continue
        tmp = rel + ".new"
        if not (disk.exists(tmp) and digest(disk.read(tmp)) == want["digest"]):
            return False
        commit(disk, tmp, rel)
        crash_point("new-replaced" if rel == ACTIVE else "previous-replaced")
        back = file_digest(disk, rel)
        if back != want["digest"]:
            raise Failed(f"{rel} read back as {back} after replacing it with {want['digest']}")
    record["current"] = dict(p["active"], installed=p["active"].get("installed") or now())
    if p["previous"] is not None:
        record["previous"] = p["previous"]
    record["pending"] = None
    record["history"] = (record["history"] + [record["current"]])[-HISTORY:]
    save_record(disk, record)
    return True


def settle(disk, record: dict, say, warn):
    """Before anything else: finish or give up an interrupted replacement,
    then remove leftovers. Returns the kind of replacement it finished."""
    leftovers = (ACTIVE + ".new", PREVIOUS + ".new", RECORD + ".new")
    for tmp in leftovers:
        refuse_shared(disk, tmp)
    finished = None
    p = record.get("pending")
    if p is not None:
        if finish(disk, record):
            finished = p["kind"]
            say(f"finished the interrupted {p['kind']} of {p['active']['digest']}")
        else:
            warn(f"the interrupted {p['kind']} of {p['active']['digest']} can't be finished; the files stay as they are")
            for key, rel in (("current", ACTIVE), ("previous", PREVIOUS)):
                d = file_digest(disk, rel)
                record[key] = known(record, d, disk.read(rel)) if d else None
            record["pending"] = None
            save_record(disk, record)
    for tmp in leftovers:
        discard(disk, tmp, say)
    return finished


def recover_previous(candidates) -> tuple[bytes, str] | None:
    """The boot.bin a running system installed, found in its closure, for an
    ESP that has none (NixOS's extraFiles deleted it)."""
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


def need_room(disk, needed: int, missing: bool) -> None:
    free = disk.free_bytes()
    if free < needed:
        raise Failed(
            f"the ESP has {free} bytes free; replacing {ACTIVE} needs {needed}. "
            + (
                f"{ACTIVE} is missing, so the Mac has no second stage: free room on the ESP and install again."
                if missing
                else f"{ACTIVE} was not changed."
            )
        )


def transition(disk, record, kind, active, active_entry, previous, previous_entry) -> None:
    """Stage, record, then commit: `previous` is the new boot.bin.previous's
    bytes, or None to leave that file as it is."""
    if previous is not None:
        put(disk, PREVIOUS, previous)
        crash_point("previous-written")
    try:
        put(disk, ACTIVE, active)
    except Failed:
        discard(disk, PREVIOUS + ".new")
        raise
    crash_point("new-written")
    record["pending"] = {"kind": kind, "active": active_entry, "previous": previous_entry}
    save_record(disk, record)
    crash_point("pending-recorded")
    if not finish(disk, record):
        raise Failed("a staged copy changed under the replacement; nothing was committed")


def install(esp, new: bytes, source=None, system=None, recover_from=(), say=print, warn=None, disk=None) -> str:
    warn = warn or (lambda m: print(f"warning: {m}", file=sys.stderr))
    disk = disk or Disk(esp)
    record = load_record(disk, warn)
    settle(disk, record, say, warn)
    disk.mkdir("m1n1")
    active = disk.read(ACTIVE) if disk.exists(ACTIVE) else None
    new_digest = digest(new)

    if active is not None and digest(active) == new_digest:
        # An interrupted run may have renamed it in but not recorded it.
        current = record.get("current") or {}
        if current.get("digest") != new_digest:
            record["current"] = dict(known(record, new_digest, new), source=source, system=system, installed=now())
            record["history"] = (record["history"] + [record["current"]])[-HISTORY:]
            save_record(disk, record)
            return f"{ACTIVE} is already {new_digest}; recorded it"
        return f"{ACTIVE} is unchanged ({new_digest})"

    held = record.get("held")
    if held is not None and active is not None and digest(active) == held["kept"]:
        if new_digest == held["replaced"]:
            warn(
                f"{ACTIVE} stays {held['kept']}, which was put back over this system's {new_digest} "
                f"({held['how']}, {held['since']}). Make an older generation the default "
                f"(nixos-rebuild boot --rollback), or run `boot_bin.py release --esp {esp}` to let the "
                f"next installation put {new_digest} back."
            )
            return f"{ACTIVE} is held at {held['kept']}"
    record["held"] = None

    new_entry = entry(new, source, system, booted=False)
    kept = disk.read(PREVIOUS) if disk.exists(PREVIOUS) else None
    # What boot.bin.previous holds now: the pending replacement expects it
    # unchanged unless it is given new bytes.
    kept_entry = known(record, digest(kept), kept) if kept is not None else None
    previous, previous_entry = None, kept_entry
    if active is None:
        if kept is None:
            recovered = recover_previous(recover_from)
            if recovered is not None and digest(recovered[0]) != new_digest:
                previous, path = recovered
                previous_entry = entry(previous, source=path, booted=None)
                say(f"{ACTIVE} was missing; keeping {path}, the copy the running system installed, as {PREVIOUS}")
            else:
                warn(f"{ACTIVE} was missing; there is no copy to keep")
    else:
        active_entry = known(record, digest(active), active)
        if active_entry.get("booted") is False and kept is not None:
            # It never started this Mac: keep the copy that did.
            say(f"{ACTIVE} ({active_entry['digest']}) never booted; {PREVIOUS} stays as it is")
        else:
            previous, previous_entry = active, active_entry
    need_room(disk, len(new) + len(previous or b"") + MARGIN, missing=active is None)

    transition(disk, record, "install", new, new_entry, previous, previous_entry)
    kept = record["previous"]["digest"] if record.get("previous") else None
    return f"{ACTIVE} is now {new_digest}" + (f"; {PREVIOUS} keeps {kept}" if kept else "")


def restore_previous(esp, say=print, warn=None, disk=None) -> str:
    warn = warn or (lambda m: print(f"warning: {m}", file=sys.stderr))
    disk = disk or Disk(esp)
    record = load_record(disk, warn)
    if settle(disk, record, say, warn) == "restore":
        return f"finished the interrupted restore: {ACTIVE} is {record['current']['digest']}"
    held = record.get("held")
    if held is not None and held["how"] == "restore-previous" and file_digest(disk, ACTIVE) == held["kept"]:
        # Done already (a retry that can't tell): swapping again would undo it.
        return (
            f"{ACTIVE} is already the restored {held['kept']}; {PREVIOUS} keeps {held['replaced']}. "
            f"`boot_bin.py release`, then an installation, puts that back."
        )
    if not disk.exists(PREVIOUS):
        raise Failed(f"there is no {PREVIOUS} to restore")
    old = disk.read(PREVIOUS)
    expected = (record.get("previous") or {}).get("digest")
    if expected and expected != digest(old):
        raise Failed(f"{PREVIOUS} is {digest(old)}, but {RECORD} says it should be {expected}; nothing was changed")
    active = disk.read(ACTIVE) if disk.exists(ACTIVE) else None
    if active is not None and digest(active) == digest(old):
        return f"{ACTIVE} and {PREVIOUS} are both {digest(old)}; there is nothing to swap"
    old_entry = known(record, digest(old), old)
    active_entry = known(record, digest(active), active) if active is not None else None
    need_room(disk, len(old) + len(active or b"") + MARGIN, missing=active is None)
    if active is not None:
        record["held"] = {"kept": digest(old), "replaced": digest(active), "how": "restore-previous", "since": now()}
    transition(disk, record, "restore", old, old_entry, active, active_entry)
    return (
        f"{ACTIVE} is now {digest(old)} again; {PREVIOUS} keeps {digest(active) if active else 'what it had'}. "
        f"Boot loader installations keep it until a different copy comes or `boot_bin.py release`; "
        f"make an older generation the default (nixos-rebuild boot --rollback) to stay with its system."
    )


def release(esp, disk=None, warn=None) -> str:
    disk = disk or Disk(esp)
    record = load_record(disk, warn or (lambda m: print(f"warning: {m}", file=sys.stderr)))
    if record.get("held") is None:
        return f"{ACTIVE} isn't held"
    held = record["held"]
    record["held"] = None
    save_record(disk, record)
    return f"the next installation may replace {held['kept']} with {held['replaced']} again"


def mark_booted(esp, say=print, warn=None, disk=None) -> str:
    """At every start: the copy at m1n1/boot.bin is the one m1n1 started."""
    warn = warn or (lambda m: print(f"warning: {m}", file=sys.stderr))
    disk = disk or Disk(esp)
    if not disk.exists(ACTIVE):
        return f"{ACTIVE} is missing"
    record = load_record(disk, warn)
    d = file_digest(disk, ACTIVE)
    p = record.get("pending") or {}
    changed = False
    for e in (record.get("current"), record.get("previous"), p.get("active"), p.get("previous")):
        if e is not None and e["digest"] == d and e.get("booted") is not True:
            e["booted"] = True
            changed = True
    current, previous = record.get("current"), record.get("previous")
    if (
        not p
        and current is not None
        and previous is not None
        and d == previous["digest"] != current["digest"]
        and not disk.exists(EXTRA_FILES_MARKER)
    ):
        # Copied over by hand from macOS (image.md's recovery): the system's
        # own copy is the one that didn't start, so hold this one.
        record["held"] = {"kept": d, "replaced": current["digest"], "how": "copied from boot.bin.previous", "since": now()}
        record["current"] = dict(previous)
        warn(f"{ACTIVE} is {PREVIOUS}'s copy; boot loader installations keep it until `boot_bin.py release`")
        changed = True
    if not changed:
        return f"{ACTIVE} ({d}) is recorded"
    save_record(disk, record)
    return f"{ACTIVE} ({d}) booted"


def status(esp, disk=None) -> dict:
    disk = disk or Disk(esp)
    record = load_record(disk, lambda m: None)
    files = {name: file_digest(disk, name) for name in (ACTIVE, PREVIOUS)}
    consistent = files[ACTIVE] == (record.get("current") or {}).get("digest") and (
        record.get("previous") is None or files[PREVIOUS] == record["previous"]["digest"]
    )
    return {"files": files, "record": record, "consistent": consistent and not record.get("pending")}


def forget_extra_files(esp, say=print, disk=None) -> str:
    """NixOS's systemd-boot builder deletes every file it once copied with
    extraFiles, as its marker under EFI/nixos/.extra-files lists them, and
    syncs that before extraInstallCommands run. Removing boot.bin's marker
    first keeps boot.bin in place for `install` to replace."""
    disk = disk or Disk(esp)
    if not disk.exists(EXTRA_FILES_MARKER):
        return "no extraFiles marker for boot.bin"
    disk.unlink(EXTRA_FILES_MARKER)
    disk.sync_dir(parent(EXTRA_FILES_MARKER))
    disk.rmdir_if_empty(parent(EXTRA_FILES_MARKER))
    return f"removed {EXTRA_FILES_MARKER}: NixOS no longer deletes {ACTIVE}"


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    sub = parser.add_subparsers(dest="command", required=True)
    i = sub.add_parser("install")
    i.add_argument("--esp", type=Path, required=True)
    i.add_argument("--source", type=Path, required=True)
    i.add_argument("--system")
    i.add_argument("--recover-from", action="append", default=[])
    for name in ("restore-previous", "release", "mark-booted", "status", "forget-extra-files"):
        sub.add_parser(name).add_argument("--esp", type=Path, required=True)
    args = parser.parse_args(argv)
    say = lambda m: print(f"sekka: {m}")  # noqa: E731
    try:
        if args.command == "install":
            source = args.source.resolve()
            say(install(args.esp, source.read_bytes(), str(source), args.system, args.recover_from, say))
        elif args.command == "restore-previous":
            say(restore_previous(args.esp, say))
        elif args.command == "release":
            say(release(args.esp))
        elif args.command == "mark-booted":
            # Never fails a boot: what it can't record now, it records next time.
            try:
                say(mark_booted(args.esp, say))
            except (Failed, OSError) as e:
                print(f"sekka: warning: {e}", file=sys.stderr)
        elif args.command == "forget-extra-files":
            say(forget_extra_files(args.esp, say))
        else:
            print(json.dumps(status(args.esp), indent=2, sort_keys=True))
    except Failed as e:
        print(f"sekka: {e}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
