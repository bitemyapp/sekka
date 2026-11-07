"""The m1n1/boot.bin replacement (boot_bin.py), two ways:

- against a temporary directory through the real system calls, with an
  interruption at every named step (an exception, as a crash of the
  script);
- against FatModel, a model of what Linux's vfat writes when (below), with
  every state a power cut can leave on the disk after every system call.

Run: python3 -m unittest test_boot_bin."""

import contextlib
import io
import itertools
import json
import tempfile
import unittest
from pathlib import Path
from unittest import mock

import boot_bin as bb

A, B, C = b"m1n1 A" * 1000, b"m1n1 B" * 1200, b"m1n1 C" * 900


def name(data):
    """A copy's name in messages: A, B, C, "empty" or None."""
    return {A: "A", B: "B", C: "C", b"": "empty"}.get(data, None if data is None else "other")


class Interrupted(Exception):
    pass


class BootBin(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.esp = Path(self.tmp.name)
        self.messages = []
        self.warnings = []

    def tearDown(self):
        self.tmp.cleanup()

    def install(self, data, **kw):
        return bb.install(self.esp, data, source=f"/nix/store/x-{data[:6].decode()}", system="/nix/store/sys",
                          say=self.messages.append, warn=self.warnings.append, **kw)

    def booted(self):
        """A start of the Mac with the active copy (sekka-boot-bin.service)."""
        return bb.mark_booted(self.esp, warn=self.warnings.append)

    def restore(self):
        return bb.restore_previous(self.esp, say=self.messages.append, warn=self.warnings.append)

    def active(self):
        return (self.esp / bb.ACTIVE).read_bytes()

    def previous(self):
        p = self.esp / bb.PREVIOUS
        return p.read_bytes() if p.exists() else None

    def record(self):
        return json.loads((self.esp / bb.RECORD).read_bytes())

    def leftovers(self):
        return sorted(str(p.relative_to(self.esp)) for p in self.esp.rglob("*.new"))

    def test_first_install_has_no_previous(self):
        self.install(A)
        self.assertEqual(self.active(), A)
        self.assertIsNone(self.previous())
        r = self.record()
        self.assertEqual(r["current"]["digest"], bb.digest(A))
        self.assertIs(r["current"]["booted"], False)
        self.assertIsNone(r["previous"])
        self.assertIsNone(r["pending"])
        self.assertEqual(len(r["history"]), 1)
        # Canonical, as the contract's documents are.
        self.assertEqual((self.esp / bb.RECORD).read_bytes(), bb.canonical(r))

    def test_unchanged_writes_nothing(self):
        self.install(A)
        before = {p: p.stat().st_mtime_ns for p in self.esp.rglob("*") if p.is_file()}
        self.assertIn("unchanged", self.install(A))
        after = {p: p.stat().st_mtime_ns for p in self.esp.rglob("*") if p.is_file()}
        self.assertEqual(before, after)

    def test_update_keeps_the_copy_that_booted(self):
        self.install(A)
        self.booted()
        self.install(B)
        self.assertEqual((self.active(), self.previous()), (B, A))
        r = self.record()
        self.assertEqual((r["current"]["digest"], r["previous"]["digest"]), (bb.digest(B), bb.digest(A)))
        self.assertIs(r["previous"]["booted"], True)
        # Two installations without a start in between: B never booted, so
        # the copy kept is still A, the one that did.
        self.install(C)
        self.assertEqual((self.active(), self.previous()), (C, A))
        self.booted()
        self.install(B)
        self.assertEqual((self.active(), self.previous()), (B, C))
        self.assertEqual([h["digest"] for h in self.record()["history"]], [bb.digest(x) for x in (A, B, C, B)])
        self.assertEqual(self.leftovers(), [])

    def test_a_switch_marks_nothing(self):
        """switch-to-configuration starts sekka-boot-bin.service when it is
        new, after installing the boot loader: the copy at boot.bin then
        never booted, and two installations before a restart keep A."""
        self.install(A)
        self.booted()
        self.install(B)
        with tempfile.TemporaryDirectory() as tmp:
            systems = Path(tmp)
            for n in ("one", "two"):
                (systems / n).mkdir()
            (systems / "booted").symlink_to(systems / "one")
            (systems / "current").symlink_to(systems / "two")
            mark = ["mark-booted", "--esp", str(self.esp),
                    "--booted-system", str(systems / "booted"), "--current-system", str(systems / "current")]
            out = io.StringIO()
            with contextlib.redirect_stdout(out):
                self.assertEqual(bb.main(mark), 0)
            self.assertIn("isn't a start", out.getvalue())
            self.assertIs(self.record()["current"]["booted"], False)
            self.install(C)
            self.assertEqual((self.active(), self.previous()), (C, A))
            # At a start the two are one system, and C is marked.
            (systems / "current").unlink()
            (systems / "current").symlink_to(systems / "one")
            with contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(bb.main(mark), 0)
            self.assertIs(self.record()["current"]["booted"], True)

    def test_rollback_installs_the_older_copy(self):
        # An older generation's boot loader installation brings its boot.bin back.
        self.install(A)
        self.booted()
        self.install(B)
        self.booted()
        self.install(A)
        self.assertEqual((self.active(), self.previous()), (A, B))

    def test_a_previous_copy_equal_to_boot_bin_is_not_staged(self):
        # boot.bin.previous already holds boot.bin's copy: here after a
        # rollback to a generation that installed boot.bin with extraFiles.
        self.install(A)
        self.booted()
        self.install(B)
        self.booted()
        (self.esp / bb.ACTIVE).write_bytes(A)
        (self.esp / bb.EXTRA_FILES_MARKER).parent.mkdir(parents=True, exist_ok=True)
        (self.esp / bb.EXTRA_FILES_MARKER).write_bytes(b"")
        self.booted()
        self.assertEqual(self.previous(), A)
        self.install(C)
        self.assertEqual((self.active(), self.previous()), (C, A))
        self.assertEqual(self.leftovers(), [])
        # Without FIBMAP, a leftover with its target's bytes would count as
        # sharing its clusters, and every installation would be refused.
        with mock.patch.object(bb.Disk, "first_block", return_value=None):
            self.install(B)
        self.assertEqual((self.active(), self.previous()), (B, A))

    def test_restore_previous_swaps_and_holds(self):
        self.install(A)
        self.booted()
        self.install(B)
        self.booted()
        self.restore()
        self.assertEqual((self.active(), self.previous()), (A, B))
        r = self.record()
        self.assertEqual((r["current"]["digest"], r["previous"]["digest"]), (bb.digest(A), bb.digest(B)))
        self.assertEqual((r["held"]["kept"], r["held"]["replaced"]), (bb.digest(A), bb.digest(B)))
        # The default generation's next installation would put B back.
        self.assertIn("held", self.install(B))
        self.assertEqual(self.active(), A)
        self.assertTrue(any("release" in w for w in self.warnings))
        # A different copy (a later fix) is installed, keeping A, which booted.
        self.booted()
        self.install(C)
        self.assertEqual((self.active(), self.previous()), (C, A))
        self.assertIsNone(self.record()["held"])

    def test_release_lets_the_held_copy_go(self):
        self.install(A)
        self.booted()
        self.install(B)
        self.booted()
        self.restore()
        bb.release(self.esp, warn=self.warnings.append)
        self.install(B)
        self.assertEqual((self.active(), self.previous()), (B, A))

    def test_a_copy_from_macos_is_held(self):
        # image.md's recovery: from macOS, boot.bin.previous copied over boot.bin.
        self.install(A)
        self.booted()
        self.install(B)
        (self.esp / bb.ACTIVE).write_bytes(A)
        self.booted()
        r = self.record()
        self.assertEqual((r["current"]["digest"], r["held"]["replaced"]), (bb.digest(A), bb.digest(B)))
        self.assertIn("held", self.install(B))
        self.assertEqual(self.active(), A)

    def test_restore_refuses_a_changed_previous(self):
        self.install(A)
        self.booted()
        self.install(B)
        (self.esp / bb.PREVIOUS).write_bytes(b"tampered")
        with self.assertRaises(bb.Failed):
            self.restore()
        self.assertEqual(self.active(), B)

    def test_history_is_bounded(self):
        for i in range(bb.HISTORY + 5):
            self.install(b"m1n1 %d" % i)
        self.assertEqual(len(self.record()["history"]), bb.HISTORY)

    def interrupted(self, run, point):
        def stop(name):
            if name == point:
                raise Interrupted(name)

        with mock.patch.object(bb, "crash_point", stop), self.assertRaises(Interrupted):
            run()

    def test_every_interruption_leaves_a_working_copy(self):
        points = ["previous-written", "new-written", "pending-recorded", "previous-replaced", "new-replaced"]
        for point in points:
            with self.subTest(point=point):
                self.tearDown()
                self.setUp()
                self.install(A)
                self.booted()
                self.interrupted(lambda: self.install(B), point)
                # The second stage is always one complete copy, A or B, and
                # A is kept from the moment B can be active.
                self.assertIn(self.active(), (A, B))
                if self.active() == B:
                    self.assertEqual(self.previous(), A)
                # The next installation finishes the job and cleans up.
                self.install(B)
                self.assertEqual((self.active(), self.previous()), (B, A))
                r = self.record()
                self.assertIsNone(r["pending"])
                self.assertEqual(r["current"]["digest"], bb.digest(B))
                self.assertEqual(self.leftovers(), [])

    def test_every_interruption_of_a_restore_keeps_both_copies(self):
        points = ["previous-written", "new-written", "pending-recorded", "new-replaced", "previous-replaced"]
        for point in points:
            for retry in ("restore", "install B"):
                with self.subTest(point=point, retry=retry):
                    self.tearDown()
                    self.setUp()
                    self.install(A)
                    self.booted()
                    self.install(B)
                    self.booted()
                    self.interrupted(self.restore, point)
                    self.assertIn(self.active(), (A, B))
                    # Both copies are still on the ESP, under some name.
                    files = {p.read_bytes() for p in (self.esp / "m1n1").iterdir()}
                    self.assertTrue({A, B} <= files, point)
                    if retry == "restore":
                        self.restore()
                        self.assertEqual((self.active(), self.previous()), (A, B))
                    else:
                        # The default generation's installation finishes a
                        # recorded restore, and then keeps it; before the
                        # record, the restore never began.
                        self.install(B)
                        started = point not in ("previous-written", "new-written")
                        self.assertEqual((self.active(), self.previous()), (A, B) if started else (B, A))
                    self.assertTrue(bb.status(self.esp)["consistent"])
                    self.assertEqual(self.leftovers(), [])

    def test_a_bad_read_back_leaves_the_active_copy(self):
        self.install(A)
        real = bb.Disk.read

        def corrupt(disk, rel):
            data = real(disk, rel)
            return b"x" + data[1:] if rel == bb.ACTIVE + ".new" else data

        with mock.patch.object(bb.Disk, "read", corrupt), self.assertRaises(bb.Failed):
            self.install(B)
        self.assertEqual(self.active(), A)
        self.assertEqual(self.leftovers(), [])

    def test_a_full_esp_changes_nothing(self):
        self.install(A)
        with mock.patch.object(bb.Disk, "free_bytes", return_value=4096), self.assertRaises(bb.Failed) as e:
            self.install(B)
        self.assertIn("was not changed", str(e.exception))
        self.assertEqual(self.active(), A)
        self.assertIsNone(self.previous())

    def test_unreadable_records_are_rebuilt(self):
        for text in ["{not json", "[]", "null", '"x"', "{}", "\xff",
                     json.dumps(dict(bb.empty_record(), history=[1])),
                     json.dumps(dict(bb.empty_record(), current={"digest": "sha256:x"})),
                     json.dumps(dict(bb.empty_record(), pending=[])),
                     json.dumps(dict(bb.empty_record(), held={"kept": 1}))]:
            with self.subTest(record=text):
                self.tearDown()
                self.setUp()
                self.install(A)
                (self.esp / bb.RECORD).write_bytes(text.encode("utf-8", "surrogateescape") if text != "\xff" else b"\xff")
                self.install(B)
                self.assertTrue(self.warnings)
                self.assertEqual((self.active(), self.previous()), (B, A))
                self.assertEqual(self.record()["current"]["digest"], bb.digest(B))
                self.assertIn("booted", bb.mark_booted(self.esp, warn=self.warnings.append))
                self.assertTrue(bb.status(self.esp)["consistent"])

    def test_a_truncated_record_is_its_staged_copy(self):
        # fsck.vfat truncated the record as its staged copy was renamed over
        # it (sharing clusters): the staged copy is the newer record.
        self.install(A)
        self.booted()
        self.install(B)
        self.booted()
        self.restore()
        record = self.esp / bb.RECORD
        (self.esp / bb.STAGED_RECORD).write_bytes(record.read_bytes())
        record.write_bytes(b"")
        self.assertEqual(bb.load_record(bb.Disk(self.esp), self.warnings.append)["held"]["kept"], bb.digest(A))
        # A retry of the restore sees it done, rather than swapping back.
        self.assertIn("already the restored", self.restore())
        self.assertEqual((self.active(), self.previous()), (A, B))
        self.assertEqual(self.record()["held"]["kept"], bb.digest(A))
        self.assertEqual(self.leftovers(), [])

    def test_missing_active_copy_recovers_the_running_systems(self):
        # boot.bin gone from the ESP (NixOS's extraFiles cleanup, before
        # forget-extra-files): the running system's copy is kept.
        store = self.esp.parent / "store-boot.bin"
        store.write_bytes(A)
        with mock.patch.object(bb, "recover_previous", return_value=(A, str(store))):
            self.install(B, recover_from=["/run/current-system"])
        self.assertEqual((self.active(), self.previous()), (B, A))
        self.assertEqual(self.record()["previous"]["source"], str(store))

    def test_the_space_check_counts_the_recovered_copy_and_says_boot_bin_is_missing(self):
        store = self.esp.parent / "store-boot.bin"
        store.write_bytes(A)
        free = len(B) + bb.MARGIN + 100  # enough for B alone, not for A too
        with mock.patch.object(bb, "recover_previous", return_value=(A, str(store))), \
                mock.patch.object(bb.Disk, "free_bytes", return_value=free), self.assertRaises(bb.Failed) as e:
            self.install(B, recover_from=["/run/current-system"])
        self.assertIn("is missing", str(e.exception))
        self.assertNotIn("was not changed", str(e.exception))

    def test_status_reports_consistency(self):
        self.install(A)
        self.booted()
        self.install(B)
        self.assertTrue(bb.status(self.esp)["consistent"])
        (self.esp / bb.ACTIVE).write_bytes(C)
        self.assertFalse(bb.status(self.esp)["consistent"])

    def test_forget_extra_files_keeps_boot_bin_through_nixos_builder(self):
        """NixOS's systemd-boot builder (systemd-boot-builder.py,
        remove_extra_files, at this Nixpkgs) deletes what its markers list;
        after forget-extra-files it leaves boot.bin and everything else."""

        def remove_extra_files(boot: Path):
            extra = boot / "EFI/nixos/.extra-files"
            for root, _, files in extra.walk(top_down=False):
                actual = boot / root.relative_to(extra)
                for file in files:
                    (actual / file).unlink(missing_ok=True)
                    (root / file).unlink()
                if not list(actual.iterdir()):
                    actual.rmdir()
                root.rmdir()
            extra.mkdir(parents=True, exist_ok=True)

        def esp_from_extra_files():
            (self.esp / "m1n1").mkdir(exist_ok=True)
            (self.esp / bb.ACTIVE).write_bytes(A)
            for marker in (bb.EXTRA_FILES_MARKER, "EFI/nixos/.extra-files/other.efi"):
                (self.esp / marker).parent.mkdir(parents=True, exist_ok=True)
                (self.esp / marker).write_bytes(b"")
            (self.esp / "other.efi").write_bytes(b"other")

        esp_from_extra_files()
        remove_extra_files(self.esp)
        self.assertFalse((self.esp / bb.ACTIVE).exists(), "the control: NixOS deletes boot.bin")
        for p in sorted(self.esp.rglob("*"), reverse=True):
            p.rmdir() if p.is_dir() else p.unlink()
        esp_from_extra_files()
        self.assertIn("removed", bb.forget_extra_files(self.esp))
        remove_extra_files(self.esp)
        self.assertEqual(self.active(), A)
        self.assertFalse((self.esp / "other.efi").exists())
        self.assertIn("no extraFiles marker", bb.forget_extra_files(self.esp))
        self.install(B)
        self.assertEqual((self.active(), self.previous()), (B, A))


# The FAT model ----------------------------------------------------------------

DELETED = ("<deleted>", 0, 0)


class FatModel:
    """Linux's vfat (fs/fat and fs/buffer.c in linux-aurora 7.1.12), as far as
    it decides what a power cut leaves on the disk, behind boot_bin.Disk's
    interface.

    A file is a directory entry (name, first cluster, size) in a directory
    sector, a chain in the FAT and data clusters. Every change goes to memory
    first, and anything changed may reach the disk at any moment, in any
    order, any number of times; a power cut leaves each sector at one of the
    contents it had since the disk last certainly held it. So the model keeps
    the set of contents each directory sector may have on the disk, and for
    each FAT entry and data cluster the values it may have. What the kernel
    does (vfat_rename, fat_remove_entries, __fat_write_inode, fat_evict_inode,
    fat_file_fsync, mmb_fsync_noflush, sync_filesystem):

    - creating a name writes an entry with cluster 0 and size 0 in a
      directory sector; removing or renaming a name marks its entry deleted.
      Those sectors are on the directory's list of metadata buffers, which
      fsync of the directory writes;
    - a file's first cluster and size reach its entry only when its inode is
      written. rename over an existing name gives the file the target's
      entry and only marks the inode dirty. Background writeback may write
      a dirty inode into its sector at any time without writing the sector,
      after which fsync of the file finds the inode clean and writes neither;
      so the model keeps, per directory, every memory content that
      writebacks may have produced;
    - clusters are allocated and freed in the FAT, which every fsync and
      syncfs writes;
    - a file whose last name is gone is evicted, freeing its clusters, when
      its last open descriptor closes;
    - the drive caches writes (the Mac's NVMe: queue/write_cache is "write
      back"), and only a flush makes them durable. Every fsync ends in one
      (fat_file_fsync, blkdev_issue_flush);
    - syncfs writes every inode and every sector but flushes nothing (vfat
      has no ->sync_fs; sync_blockdev only waits for the writes): what it
      wrote may be lost, or stored after later writes, until the next
      fsync's flush, after which the disk holds what it wrote or anything
      memory had since.

    Every name takes one entry; `per_sector` entries share a directory
    sector (128 on a 4 KiB-sector ESP like the Mac's, 16 at 512 bytes; 1
    puts every entry in its own sector, the worst case)."""

    CLUSTER = 64

    def __init__(self, per_sector, clusters=4096):
        self.per_sector = per_sector
        self.dirs = {"": [[]]}  # directory -> memory variants of its entry list
        self.dir_disk = {}  # (directory, sector) -> [contents the disk may hold]
        self.dir_dirty = {}  # directory -> sectors on its list of metadata buffers
        # (directory, sector) -> what syncfs wrote, then memory's later
        # contents: what the disk may hold once the next flush comes.
        self.cached = {}
        self.inodes = {}  # id -> {dir, pos, start, size, nlink, opens}
        self.names = {}  # (directory, name) -> inode id
        self.pending = set()  # inodes whose entry may not hold their values
        self.fat = [0] * clusters
        self.fat_disk = [[0] for _ in range(clusters)]
        self.data = {}
        self.data_disk = {}
        self.next_id = 1
        self.hint = 2
        self.snapshots = []
        self.recording = False

    # The disk's possible contents ------------------------------------------

    def _sector(self, v, s):
        content = list(v[s * self.per_sector : (s + 1) * self.per_sector])
        return tuple(content + [None] * (self.per_sector - len(content)))

    def _sectors(self, v):
        return range(max(1, -(-len(v) // self.per_sector)))

    def _note_dir(self, d):
        for v in self.dirs[d]:
            for s in self._sectors(v):
                content = self._sector(v, s)
                seen = self.dir_disk.setdefault((d, s), [(None,) * self.per_sector])
                if content not in seen:
                    seen.append(content)
                after = self.cached.get((d, s))
                if after is not None and content not in after:
                    after.append(content)

    def _flush(self):
        """blkdev_issue_flush: what syncfs wrote into the drive's cache is
        durable, unless background writeback overwrote it since."""
        for key, contents in self.cached.items():
            self.dir_disk[key] = list(contents)
        self.cached.clear()

    def _set_fat(self, c, value):
        self.fat[c] = value
        if value not in self.fat_disk[c]:
            self.fat_disk[c].append(value)

    def _fat_durable(self):
        for c, value in enumerate(self.fat):
            self.fat_disk[c] = [value]

    def _apply(self, i, variants=None):
        """Variants with inode i's values in its entry (a writeback)."""
        node = self.inodes[i]
        out = []
        for v in self.dirs[node["dir"]] if variants is None else variants:
            w = list(v)
            name = w[node["pos"]][0]
            w[node["pos"]] = (name, node["start"], node["size"])
            out.append(w)
        return out

    def _dedup(self, variants):
        out = []
        for v in variants:
            if v not in out:
                out.append(v)
        return out

    def _dirty_inode(self, i):
        node = self.inodes[i]
        if node["pos"] is None:
            return
        d = node["dir"]
        self.dirs[d] = self._dedup(self.dirs[d] + self._apply(i))
        self.pending.add(i)
        self._note_dir(d)

    def _change_dir(self, d, change, sectors):
        self.dirs[d] = self._dedup([change(list(v)) for v in self.dirs[d]])
        self.dir_dirty.setdefault(d, set()).update(sectors)
        self._note_dir(d)

    def _snap(self, what):
        if self.recording:
            self.snapshots.append((what, {k: list(v) for k, v in self.dir_disk.items()},
                                   [list(v) for v in self.fat_disk], {k: list(v) for k, v in self.data_disk.items()}))

    # boot_bin.Disk's interface -----------------------------------------------

    def _split(self, rel):
        d = bb.parent(rel)
        return d, rel[len(d) + 1 :] if d else rel

    def exists(self, rel):
        if rel in self.dirs:
            return True
        return self._split(rel) in self.names

    def read(self, rel):
        node = self.inodes[self.names[self._split(rel)]]
        out, c = b"", node["start"]
        while c and len(out) < node["size"]:
            out += self.data.get(c, b"\0" * self.CLUSTER)
            c = self.fat[c] if self.fat[c] not in (0, -1) else 0
        return out[: node["size"]]

    def _slot(self, d):
        v = self.dirs[d][0]
        for pos, e in enumerate(v):
            if e is None or e == DELETED:
                return pos
        for w in self.dirs[d]:
            w.append(None)
        return len(v) - 1

    def write(self, rel, data):
        d, name = self._split(rel)
        assert (d, name) not in self.names
        pos = self._slot(d)
        self._change_dir(d, lambda v: v[:pos] + [(name, 0, 0)] + v[pos + 1 :], {pos // self.per_sector})
        i = self.next_id
        self.next_id += 1
        self.inodes[i] = {"dir": d, "pos": pos, "start": 0, "size": 0, "nlink": 1, "opens": 0}
        self.names[(d, name)] = i
        self._snap(f"create {rel}")
        chain = []
        for k in range(0, len(data), self.CLUSTER):
            c = self.hint
            while self.fat[c] != 0:
                c = c + 1 if c + 1 < len(self.fat) else 2
            self.hint = c + 1
            self._set_fat(c, -1)
            if chain:
                self._set_fat(chain[-1], c)
            chain.append(c)
            chunk = data[k : k + self.CLUSTER]
            self.data[c] = chunk
            self.data_disk.setdefault(c, [None])
            if chunk not in self.data_disk[c]:
                self.data_disk[c].append(chunk)
        self.inodes[i].update(start=chain[0] if chain else 0, size=len(data))
        self._dirty_inode(i)
        self._snap(f"write {rel}")
        # fsync: data, the FAT, and the entry if the inode is still dirty;
        # then the flush.
        for c in chain:
            self.data_disk[c] = [self.data[c]]
        self._fat_durable()
        self.dirs[d] = self._dedup(self._apply(i))
        self.pending.discard(i)
        self._note_dir(d)
        self._flush()
        self._snap(f"fsync {rel}")

    def hold(self, rel):
        i = self.names[self._split(rel)]
        self.inodes[i]["opens"] += 1
        return i

    def release(self, i):
        self.inodes[i]["opens"] -= 1
        self._evict_if_gone(i)
        self._snap(f"close inode {i}")

    def _evict_if_gone(self, i):
        node = self.inodes[i]
        if node["nlink"] == 0 and node["opens"] == 0:
            c = node["start"]
            while c and c != -1:
                nxt = self.fat[c]
                self._set_fat(c, 0)
                c = nxt
            del self.inodes[i]

    def rename(self, src, dst):
        d, sname = self._split(src)
        d2, dname = self._split(dst)
        assert d == d2, "the model renames within a directory"
        i = self.names.pop((d, sname))
        old_pos = self.inodes[i]["pos"]
        target = self.names.get((d, dname))
        if target is not None:
            new_pos = self.inodes[target]["pos"]
            self.inodes[target].update(pos=None, nlink=0)
            self.pending.discard(target)
        else:
            new_pos = self._slot(d)
            self._change_dir(d, lambda v: v[:new_pos] + [(dname, 0, 0)] + v[new_pos + 1 :],
                             {new_pos // self.per_sector})
        self.inodes[i]["pos"] = new_pos
        self.names[(d, dname)] = i
        self._dirty_inode(i)
        self._change_dir(d, lambda v: v[:old_pos] + [DELETED] + v[old_pos + 1 :], {old_pos // self.per_sector})
        if target is not None:
            self._evict_if_gone(target)
        self._snap(f"rename {src} {dst}")

    def unlink(self, rel):
        d, name = self._split(rel)
        i = self.names.pop((d, name))
        pos = self.inodes[i]["pos"]
        self.inodes[i].update(pos=None, nlink=0)
        self.pending.discard(i)
        self._change_dir(d, lambda v: v[:pos] + [DELETED] + v[pos + 1 :], {pos // self.per_sector})
        self._evict_if_gone(i)
        self._snap(f"unlink {rel}")

    def mkdir(self, rel):
        if rel not in self.dirs:
            self.dirs[rel] = [[]]

    def rmdir_if_empty(self, rel):
        pass

    def sync_dir(self, d):
        """fsync of a directory: its own changed sectors and the FAT, then
        the flush."""
        for s in self.dir_dirty.pop(d, set()):
            self.dir_disk[(d, s)] = self._dedup([self._sector(v, s) for v in self.dirs[d]])
            self.cached.pop((d, s), None)
        self._fat_durable()
        self._flush()
        self._snap(f"fsync {d or '/'}/")

    def sync_all(self):
        """syncfs: every inode into its entry and every sector written, into
        the drive's cache; none of it durable before the next flush."""
        for i in list(self.pending):
            if self.inodes.get(i, {}).get("pos") is not None:
                d = self.inodes[i]["dir"]
                self.dirs[d] = self._dedup(self._apply(i))
        self.pending.clear()
        for d, variants in self.dirs.items():
            assert len(variants) == 1, variants
            for s in self._sectors(variants[0]):
                content = self._sector(variants[0], s)
                seen = self.dir_disk.setdefault((d, s), [(None,) * self.per_sector])
                if content not in seen:
                    seen.append(content)
                self.cached[(d, s)] = [content]
        self.dir_dirty.clear()
        # The FAT and the data: written too, and as uncertain as before
        # until a flush (the model's values only ever grow until then).
        self._snap("syncfs")

    def quiesce(self):
        """Long after the last change, with a flush since: the disk holds
        memory's content (only to set a test's ESP up)."""
        self.sync_all()
        self._flush()
        self._fat_durable()
        for c, chunk in self.data.items():
            self.data_disk[c] = [chunk]

    def free_bytes(self):
        return self.fat.count(0) * self.CLUSTER

    def first_block(self, rel):
        return self.inodes[self.names[self._split(rel)]]["start"]

    def is_fat(self):
        return True

    # After a power cut -------------------------------------------------------

    @staticmethod
    def states(snapshot, d):
        """Every content directory d may have on the disk: its sectors' choices."""
        _, dir_disk, _, _ = snapshot
        sectors = sorted(s for (dd, s) in dir_disk if dd == d)
        for choice in itertools.product(*(dir_disk[(d, s)] for s in sectors)):
            yield [e for sector in choice for e in sector]

    @classmethod
    def certain_read(cls, snapshot, start, size):
        """The file's bytes if the disk certainly holds them, else None."""
        _, _, fat_disk, data_disk = snapshot
        out, c = b"", start
        while len(out) < size:
            if not c or c == -1 or len(data_disk.get(c, [])) != 1 or len(fat_disk[c]) != 1 or fat_disk[c][0] == 0:
                return None
            out += data_disk[c][0]
            c = fat_disk[c][0]
        if c != -1:
            return None
        return out[:size]

    @classmethod
    def files(cls, snapshot, entries):
        """name -> bytes certainly on the disk (None if not certain), and the
        names that share a first cluster."""
        live = [e for e in entries if e is not None and e != DELETED]
        out = {}
        for name, start, size in live:
            out.setdefault(name, []).append(cls.certain_read(snapshot, start, size) if size else b"")
        starts = [start for _, start, _ in live if start]
        shared = {name for name, start, _ in live if start and starts.count(start) > 1}
        return out, shared

    @staticmethod
    def fsck(choice):
        """What fsck.vfat -a, which systemd runs on the ESP before mounting it
        at every start (/boot is fsck pass 2), may make of a state: of two
        names sharing clusters it truncates one, which here is to nothing, as
        they share their first. Which one depends on the order it meets them,
        so each is a state the next start may find. Without sharing, the
        state as it is."""
        owners = {}
        for d, entries in choice.items():
            for pos, e in enumerate(entries):
                if e is not None and e != DELETED and e[1]:
                    owners.setdefault(e[1], []).append((d, pos))
        groups = [g for g in owners.values() if len(g) > 1]
        assert all(len(g) == 2 for g in groups), groups
        for picks in itertools.product(*groups):
            out = {d: list(entries) for d, entries in choice.items()}
            for d, pos in picks:
                out[d][pos] = (out[d][pos][0], 0, 0)
            yield out

    def mounted(self, snapshot, choice):
        """A new model holding one state the disk may be in: `choice` picks
        each directory's content, and each FAT entry and cluster takes its
        latest value."""
        _, dir_disk, fat_disk, data_disk = snapshot
        m = FatModel(self.per_sector, len(self.fat))
        m.dirs = {}
        for d, entries in choice.items():
            m.dirs[d] = [[e for e in entries]]
        m.fat = [v[-1] for v in fat_disk]
        m.fat_disk = [[v] for v in m.fat]
        m.data = {c: v[-1] for c, v in data_disk.items() if v[-1] is not None}
        m.data_disk = {c: [v] for c, v in m.data.items()}
        for d, (entries,) in m.dirs.items():
            for pos, e in enumerate(entries):
                if e is not None and e != DELETED:
                    i = m.next_id
                    m.next_id += 1
                    m.inodes[i] = {"dir": d, "pos": pos, "start": e[1], "size": e[2], "nlink": 1, "opens": 0}
                    m.names[(d, e[0])] = i
            for s in m._sectors(entries):
                m.dir_disk[(d, s)] = [m._sector(entries, s)]
        m.hint = max([c for c, v in enumerate(m.fat) if v] + [1]) + 1
        return m


class OnFat(unittest.TestCase):
    """Every state a power cut can leave, after every system call of an
    installation or a restore: in every one m1n1/boot.bin is one complete
    copy, and once the new copy can be there the replaced one is certainly
    kept. Names share clusters only as a file and the staged copy renamed
    over it, through a write-back during the rename. From every state, the
    next run ends with the new copy active and the old one kept, or, where
    names share clusters, refuses and changes nothing."""

    # A file and the staged copy renamed over it: the only names that may
    # come to share clusters.
    PAIRS = {frozenset({"boot.bin", "boot.bin.new"}), frozenset({"boot.bin.previous", "boot.bin.previous.new"})}

    def setUp(self):
        # The model's ESP is a few hundred KiB.
        margin = mock.patch.object(bb, "MARGIN", 1024)
        margin.start()
        self.addCleanup(margin.stop)

    def esp(self, per_sector, booted=(A,), active=A, previous=None):
        m = FatModel(per_sector)
        for data in booted:
            bb.install(None, data, say=lambda _: None, warn=lambda _: None, disk=m)
            bb.mark_booted(None, warn=lambda _: None, disk=m)
        m.quiesce()
        self.assertEqual(m.read(bb.ACTIVE), active)
        if previous is not None:
            self.assertEqual(m.read(bb.PREVIOUS), previous)
        m.snapshots, m.recording = [], True
        return m

    def check(self, m, allowed, kept_when, label, always=()):
        """Each snapshot: boot.bin certainly one of `allowed`; when it may be
        kept_when[0], boot.bin.previous certainly kept_when[1]; every copy in
        `always` certainly under some name; shared clusters only in PAIRS."""
        states = []
        for snap in m.snapshots:
            seen = set()
            for m1n1 in FatModel.states(snap, "m1n1"):
                key = tuple(m1n1)
                if key in seen:
                    continue
                seen.add(key)
                files, shared = FatModel.files(snap, m1n1)
                where = f"{label}: after {snap[0]}, with m1n1/ holding {[e for e in m1n1 if e]}"
                active = files.get("boot.bin")
                self.assertIsNotNone(active, f"{where}: no boot.bin")
                self.assertEqual(len(active), 1, f"{where}: boot.bin twice")
                self.assertTrue(active[0] in allowed, f"{where}: boot.bin isn't a complete copy")
                if kept_when and active[0] == kept_when[0]:
                    self.assertTrue(files.get("boot.bin.previous") == [kept_when[1]],
                                    f"{where}: the replaced copy isn't certainly kept")
                certain = {x for v in files.values() for x in v if x is not None}
                for copy in always:
                    self.assertIn(copy, certain, f"{where}: a copy is lost")
                self.assertTrue(not shared or frozenset(shared) in self.PAIRS, f"{where}: {shared} share clusters")
                states.append((snap, m1n1))
        return states

    def recover(self, m, states, retry, label):
        """From each state, mounted again as the next start mounts it, after
        fsck.vfat (FatModel.fsck): `retry` finishes. Mounted without the
        check, a state where names share clusters is refused, leaving
        boot.bin as it was ("refused"); each such state also finishes once
        checked ("after fsck")."""
        outcomes, tried = set(), set()
        for snap, m1n1 in states:
            for sekka in FatModel.states(snap, "sekka"):
                raw = {"": [], "m1n1": m1n1, "sekka": sekka}
                for choice in [raw, *FatModel.fsck(raw)]:
                    again = m.mounted(snap, choice)
                    starts = [again.inodes[i]["start"] for i in again.names.values() if again.inodes[i]["start"]]
                    shared = len(starts) != len(set(starts))
                    # States the next run can't tell apart: the same files
                    # with the same bytes, sharing the same way.
                    key = (shared, tuple(sorted((d, n, bb.digest(again.read(f"{d}/{n}"))) for (d, n) in again.names)))
                    if key in tried:
                        continue
                    tried.add(key)
                    before = again.read(bb.ACTIVE) if again.exists(bb.ACTIVE) else None
                    again.recording = True
                    try:
                        retry(again)
                    except bb.Failed as e:
                        self.assertTrue(shared, f"{label}: {e}")
                        self.assertIn("share their clusters", str(e))
                        self.assertEqual(again.read(bb.ACTIVE), before)
                        outcomes.add("refused")
                        continue
                    self.assertFalse(shared, f"{label}: a leftover sharing clusters was not refused")
                    if choice is not raw and choice != raw:
                        outcomes.add("after fsck")
                    previous = again.read(bb.PREVIOUS) if again.exists(bb.PREVIOUS) else None
                    outcomes.add((again.read(bb.ACTIVE), previous))
                    # Never worse than the state it started from: after
                    # fsck.vfat, boot.bin may be empty until it is finished.
                    self.check(again, {A, B, before}, None, f"{label}, recovering")
        self.assertEqual("refused" in outcomes, "after fsck" in outcomes, f"{label}: {outcomes}")
        return outcomes - {"refused", "after fsck"}, "after fsck" in outcomes

    def test_installing_keeps_one_complete_copy_on_any_sector_layout(self):
        for per_sector in (128, 16, 2, 1):
            with self.subTest(per_sector=per_sector):
                m = self.esp(per_sector)
                bb.install(None, B, say=lambda _: None, warn=lambda _: None, disk=m)
                states = self.check(m, {A, B}, (B, A), f"{per_sector} per sector", always=(A,))
                retry = lambda d: bb.install(None, B, say=lambda _: None, warn=lambda _: None, disk=d)  # noqa: E731
                outcomes, after_fsck = self.recover(m, states, retry, f"{per_sector} per sector")
                self.assertEqual(outcomes, {(B, A)})
                if per_sector == 1:
                    # Entries in sectors of their own: the shared state
                    # happens, and is finished once fsck.vfat has run.
                    self.assertTrue(after_fsck)

    def test_the_start_after_a_power_cut_finishes_the_replacement(self):
        """sekka-boot-bin.service at the next start (mark-booted), before any
        installation: where fsck.vfat truncated boot.bin, it is the staged
        copy again before m1n1 reads it at the start after."""
        for per_sector in (16, 1):
            with self.subTest(per_sector=per_sector):
                m = self.esp(per_sector)
                bb.install(None, B, say=lambda _: None, warn=lambda _: None, disk=m)
                states = self.check(m, {A, B}, (B, A), f"{per_sector} per sector", always=(A,))
                start = lambda d: bb.mark_booted(None, say=lambda _: None, warn=lambda _: None, disk=d)  # noqa: E731
                outcomes, after_fsck = self.recover(m, states, start, f"start, {per_sector} per sector")
                # boot.bin is one complete copy (never what fsck.vfat left
                # of it), B only with A kept; a replacement it can't finish
                # is the next installation's to do.
                names = {(name(a), name(p)) for a, p in outcomes}
                self.assertTrue(all(a in ("A", "B") for a, _ in names), names)
                self.assertTrue(all(p == "A" for a, p in names if a == "B"), names)
                self.assertIn(("B", "A"), names)
                self.assertTrue(after_fsck)

    def test_restoring_keeps_the_restored_copy(self):
        for per_sector in (128, 1):
            with self.subTest(per_sector=per_sector):
                m = self.esp(per_sector, booted=(A, B), active=B, previous=A)
                bb.restore_previous(None, say=lambda _: None, warn=lambda _: None, disk=m)
                # A, the copy being restored, is never only in memory; B,
                # the one restored away (still in the Nix store), may be an
                # unnamed chain for the instant boot.bin.previous's entry
                # changes.
                states = self.check(m, {A, B}, None, f"restore, {per_sector} per sector", always=(A,))
                retry = lambda d: bb.restore_previous(None, say=lambda _: None, warn=lambda _: None, disk=d)  # noqa: E731
                outcomes, _ = self.recover(m, states, retry, f"restore, {per_sector} per sector")
                names = {(name(a), name(p)) for a, p in outcomes}
                self.assertTrue(names <= {("A", "B"), ("A", "A")}, names)
                self.assertIn(("A", "B"), names)

    def test_the_model_catches_the_old_procedure(self):
        """The control: boot_bin.py before this model (os.replace, then an
        fsync of the directory) leaves boot.bin naming freed clusters, on
        any sector layout."""
        for per_sector in (128, 1):
            with self.subTest(per_sector=per_sector):
                m = self.esp(per_sector)

                def old_commit(disk, tmp, rel):
                    disk.rename(tmp, rel)
                    disk.sync_dir(bb.parent(rel))

                with mock.patch.object(bb, "commit", old_commit):
                    bb.install(None, B, say=lambda _: None, warn=lambda _: None, disk=m)
                with self.assertRaisesRegex(AssertionError, "isn't a complete copy"):
                    self.check(m, {A, B}, (B, A), "old procedure")

    def test_the_model_catches_freeing_before_the_flush(self):
        """The control for the drive's cache: closing the replaced file right
        after syncfs, which flushes nothing, frees its clusters while the
        entry syncfs wrote may not be on the disk yet."""
        for per_sector in (128, 1):
            with self.subTest(per_sector=per_sector):
                m = self.esp(per_sector)

                def unflushed_commit(disk, tmp, rel):
                    held = disk.hold(rel) if disk.exists(rel) else None
                    disk.rename(tmp, rel)
                    disk.sync_dir(bb.parent(rel))
                    disk.sync_all()
                    if held is not None:
                        disk.release(held)

                with mock.patch.object(bb, "commit", unflushed_commit):
                    bb.install(None, B, say=lambda _: None, warn=lambda _: None, disk=m)
                with self.assertRaisesRegex(AssertionError, "isn't a complete copy|isn't certainly kept"):
                    self.check(m, {A, B}, (B, A), "freed before the flush")


if __name__ == "__main__":
    unittest.main()
