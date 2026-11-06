"""The m1n1/boot.bin replacement against a fake ESP (a temporary directory),
including an interruption at every step. Run: python3 -m unittest test_boot_bin."""

import json
import os
import tempfile
import unittest
from pathlib import Path
from unittest import mock

import boot_bin as bb

A, B, C = b"m1n1 A" * 1000, b"m1n1 B" * 1200, b"m1n1 C" * 900


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

    def test_update_keeps_the_replaced_copy(self):
        self.install(A)
        self.install(B)
        self.assertEqual(self.active(), B)
        self.assertEqual(self.previous(), A)
        r = self.record()
        self.assertEqual((r["current"]["digest"], r["previous"]["digest"]), (bb.digest(B), bb.digest(A)))
        self.install(C)
        self.assertEqual((self.active(), self.previous()), (C, B))
        self.assertEqual([h["digest"] for h in self.record()["history"]], [bb.digest(x) for x in (A, B, C)])
        self.assertEqual(self.leftovers(), [])

    def test_rollback_installs_the_older_copy(self):
        # An older generation's boot loader installation brings its boot.bin back.
        self.install(A)
        self.install(B)
        self.install(A)
        self.assertEqual((self.active(), self.previous()), (A, B))

    def test_restore_previous_swaps(self):
        self.install(A)
        self.install(B)
        bb.restore_previous(self.esp, say=self.messages.append, warn=self.warnings.append)
        self.assertEqual((self.active(), self.previous()), (A, B))

    def test_restore_refuses_a_changed_previous(self):
        self.install(A)
        self.install(B)
        (self.esp / bb.PREVIOUS).write_bytes(b"tampered")
        with self.assertRaises(bb.Failed):
            bb.restore_previous(self.esp, warn=self.warnings.append)
        self.assertEqual(self.active(), B)

    def test_history_is_bounded(self):
        for i in range(bb.HISTORY + 5):
            self.install(b"m1n1 %d" % i)
        self.assertEqual(len(self.record()["history"]), bb.HISTORY)

    def test_every_interruption_leaves_a_working_copy(self):
        points = ["previous-written", "previous-replaced", "pending-recorded", "new-written", "new-replaced"]
        for point in points:
            with self.subTest(point=point):
                self.tearDown()
                self.setUp()
                self.install(A)

                def stop(name, point=point):
                    if name == point:
                        raise Interrupted(name)

                with mock.patch.object(bb, "crash_point", stop), self.assertRaises(Interrupted):
                    self.install(B)
                # The second stage is always one complete copy, A or B.
                self.assertIn(self.active(), (A, B))
                if point != "new-replaced":
                    self.assertEqual(self.active(), A)
                # The next installation finishes the job and cleans up.
                self.install(B)
                self.assertEqual((self.active(), self.previous()), (B, A))
                r = self.record()
                self.assertIsNone(r["pending"])
                self.assertEqual(r["current"]["digest"], bb.digest(B))
                self.assertEqual(self.leftovers(), [])

    def test_a_bad_read_back_leaves_the_active_copy(self):
        self.install(A)
        real = bb.read_back

        def corrupt(path):
            data = real(path)
            return b"x" + data[1:] if path.name == "boot.bin.new" else data

        with mock.patch.object(bb, "read_back", corrupt), self.assertRaises(bb.Failed):
            self.install(B)
        self.assertEqual(self.active(), A)
        self.assertEqual(self.leftovers(), [])

    def test_a_full_esp_changes_nothing(self):
        self.install(A)
        full = os.statvfs_result((4096, 4096, 100, 1, 1, 100, 1, 1, 0, 255, 0))
        with mock.patch.object(bb.os, "statvfs", return_value=full), self.assertRaises(bb.Failed):
            self.install(B)
        self.assertEqual(self.active(), A)
        self.assertIsNone(self.previous())

    def test_an_unreadable_record_is_rebuilt(self):
        self.install(A)
        (self.esp / bb.RECORD).write_text("{not json")
        self.install(B)
        self.assertTrue(self.warnings)
        self.assertEqual((self.active(), self.previous()), (B, A))
        self.assertEqual(self.record()["current"]["digest"], bb.digest(B))

    def test_missing_active_copy_recovers_the_running_systems(self):
        # NixOS's extraFiles cleanup deleted it just before (the first
        # installation after switching to this script).
        store = self.esp.parent / "store-boot.bin"
        store.write_bytes(A)
        with mock.patch.object(bb, "recover_previous", return_value=(A, str(store))):
            self.install(B, recover_from=["/run/current-system"])
        self.assertEqual((self.active(), self.previous()), (B, A))
        self.assertEqual(self.record()["previous"]["source"], str(store))

    def test_status_reports_consistency(self):
        self.install(A)
        self.install(B)
        self.assertTrue(bb.status(self.esp)["consistent"])
        (self.esp / bb.ACTIVE).write_bytes(C)
        self.assertFalse(bb.status(self.esp)["consistent"])


if __name__ == "__main__":
    unittest.main()
