import contextlib
import datetime
import importlib.util
import io
import json
import os
from pathlib import Path
import random
import sqlite3
import stat
import sys
import tempfile
import unittest
import zlib


spec = importlib.util.spec_from_file_location(
    "lag_history", Path(__file__).resolve().parents[1] / "nix/scheduling/lag_history.py"
)
history = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = history
spec.loader.exec_module(history)


class Clock:
    def __init__(self, now=10_000_000):
        self.now = now

    def __call__(self):
        return self.now


def record(wall, sample_id, kind="summary", run_id="run-a", **extra):
    return {"type": kind, "run_id": run_id, "sample_id": sample_id,
            "wall_utc": datetime.datetime.fromtimestamp(wall, datetime.timezone.utc).isoformat(),
            **extra}


class HistoryTest(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.directory = Path(self.temporary.name) / "history"
        self.clock = Clock()

    def store(self, **kwargs):
        return history.HistoryStore(self.directory, clock=self.clock, **kwargs)

    def records(self, **kwargs):
        return list(history.query_records(self.directory, **kwargs))

    def test_72_hour_boundary_retains_24_and_48_hour_evidence(self):
        now = self.clock.now
        ages = [72 * 3600 + 1, 72 * 3600, 48 * 3600, 24 * 3600, 0]
        with self.store() as store:
            for identifier, age in enumerate(ages):
                store.write(record(now - age, identifier))
            stats = store.maintain(now)
            self.assertEqual([item["sample_id"] for item in self.records()], [1, 2, 3, 4])
            self.assertEqual(stats["oldest_wall"], now - 72 * 3600)
            self.assertIsNone(stats["retention_notice"])
            self.clock.now += 1
            store.maintain()
            self.assertEqual([item["sample_id"] for item in self.records()], [2, 3, 4])
        self.clock.now += 73 * 3600
        with self.store() as reopened:
            self.assertEqual(reopened.stats()["record_count"], 0)
        self.assertEqual(self.records(), [])

    def test_sample_identity_deduplicates_replay_and_survives_restart(self):
        original = record(self.clock.now, 1, kind="sample", context="quiet")
        other = {"type": "metadata", "wall_utc": original["wall_utc"], "kernel": "test"}
        with self.store() as store:
            self.assertTrue(store.write(original))
            self.assertFalse(store.write({**original, "context": "prehistory", "event_id": 9}))
            self.assertTrue(store.write(record(self.clock.now, 1, kind="summary")))
            self.assertTrue(store.write(record(self.clock.now, 1, kind="sample", run_id="run-b")))
            self.assertTrue(store.write(other))
            self.assertFalse(store.write(dict(reversed(list(other.items())))))
        with self.store() as reopened:
            self.assertFalse(reopened.write(original))
            self.assertFalse(reopened.write(other))
            self.assertEqual(reopened.stats()["record_count"], 4)
        samples = self.records(kinds=["sample"])
        self.assertEqual(len(samples), 2)
        self.assertEqual(next(item for item in samples if item["run_id"] == "run-a")["context"], "quiet")

    def test_compression_order_inclusive_filters_and_reopen(self):
        now = self.clock.now
        originals = [record(now - offset, offset, kind="sample" if offset % 2 else "summary",
                            details="pressure:quiet;" * 2000) for offset in [3, 0, 2, 1]]
        tied = record(now - 2, 100, details="same timestamp")
        with self.store() as store:
            for item in [*originals, tied]:
                store.write(item)
            store.flush()
            payloads = store.connection.execute("SELECT payload FROM records").fetchall()
            decoded = [json.loads(zlib.decompress(row[0])) for row in payloads]
            self.assertCountEqual(decoded, [*originals, tied])
            self.assertLess(sum(len(row[0]) for row in payloads),
                            sum(len(json.dumps(item)) for item in originals) // 10)
            first_order = self.records()
        self.assertEqual(self.records(), first_order)
        self.assertEqual([item["wall_utc"] for item in first_order],
                         sorted(item["wall_utc"] for item in first_order))
        self.assertCountEqual(self.records(since=now - 2, until=now - 1),
                              [originals[2], originals[3], tied])
        self.assertEqual(self.records(since=now - 2, until=now - 1, kinds=["sample"]), [originals[3]])
        self.assertCountEqual(self.records(since=now - 2, until=now - 2), [originals[2], tied])
        self.assertEqual(self.records(kinds=[]), [])
        self.assertEqual(self.records(since=now + 1), [])

    def test_batch_commit_flush_close_and_read_only_stats_during_writer(self):
        with self.store() as store:
            store.write(record(self.clock.now, 1))
            self.assertEqual(history.history_stats(self.directory)["record_count"], 0)
            self.clock.now += history.COMMIT_SECONDS
            store.write(record(self.clock.now, 2))
            self.assertEqual(history.history_stats(self.directory)["record_count"], 2)
            store.write(record(self.clock.now, 3))
            store.flush()
            self.assertEqual(history.history_stats(self.directory)["record_count"], 3)
            store.write(record(self.clock.now, 4))
        self.assertEqual(history.history_stats(self.directory)["record_count"], 4)

    def test_count_bound_commits_without_clock_advancing(self):
        with self.store() as store:
            for identifier in range(history.BATCH_RECORDS):
                store.write(record(self.clock.now, identifier))
            self.assertEqual(history.history_stats(self.directory)["record_count"], history.BATCH_RECORDS)

    def test_paused_query_does_not_pin_wal_between_batches(self):
        with self.store() as store:
            for identifier in range(history.QUERY_BATCH_RECORDS + 10):
                store.write(record(self.clock.now + identifier, identifier))
            store.flush()
            stream = history.query_records(self.directory)
            self.addCleanup(stream.close)
            next(stream)
            store.write(record(self.clock.now + 1000, 1000))
            store.flush()
            checkpoint = store.connection.execute("PRAGMA wal_checkpoint(TRUNCATE)").fetchone()
            self.assertEqual(checkpoint[0], 0)
            self.assertEqual(store.stats()["wal_bytes"], 0)
            self.assertEqual(len(list(stream)), history.QUERY_BATCH_RECORDS + 10)

    def test_budget_eviction_is_explicit_durable_and_reclaims_allocation(self):
        rng = random.Random(7)
        budget = 256 * 1024
        warning = io.StringIO()
        with contextlib.redirect_stderr(warning), self.store(max_bytes=budget) as store:
            for identifier in range(400):
                store.write(record(self.clock.now - 1000 + identifier, identifier,
                                   detail=rng.randbytes(1024).hex()))
            stats = store.maintain()
            notice = stats["retention_notice"]
            self.assertIsNotNone(notice)
            self.assertEqual(notice["reason"], "byte_budget")
            self.assertGreater(notice["records_evicted"], 0)
            self.assertEqual(notice["records_evicted"] + stats["record_count"], 400)
            self.assertGreater(notice["lost_since"], self.clock.now - history.RETENTION_SECONDS)
            self.assertGreaterEqual(notice["lost_until"], notice["lost_since"])
            self.assertGreater(stats["record_count"], 0)
            self.assertLessEqual(stats["allocated_bytes"], budget)
            self.assertFalse(stats["over_budget"])
        self.assertTrue(warning.getvalue())
        with self.store(max_bytes=budget) as reopened:
            self.assertEqual(reopened.stats()["retention_notice"], notice)
        self.assertEqual(history.history_stats(self.directory)["retention_notice"], notice)

    def test_private_permissions_and_exclusive_writer_lock(self):
        self.directory.mkdir(mode=0o755)
        with self.store() as store:
            store.write(record(self.clock.now, 1))
            store.flush()
            self.assertEqual(stat.S_IMODE(self.directory.stat().st_mode), 0o700)
            for path in self.directory.iterdir():
                self.assertEqual(stat.S_IMODE(path.stat().st_mode), 0o600)
            with self.assertRaises(RuntimeError):
                self.store()
            self.assertEqual(history.history_stats(self.directory)["record_count"], 1)
        with self.store() as reopened:
            self.assertEqual(reopened.stats()["record_count"], 1)

    def test_symlinks_and_nonregular_files_are_refused(self):
        target = Path(self.temporary.name) / "target"
        target.write_text("untouched")
        for name in ["history.sqlite3", "history.lock", "history.sqlite3-wal",
                     "history.sqlite3-shm", "history.sqlite3-journal"]:
            with self.subTest(name=name):
                directory = Path(self.temporary.name) / name.replace(".", "_")
                directory.mkdir()
                (directory / name).symlink_to(target)
                with self.assertRaises((RuntimeError, OSError)):
                    history.HistoryStore(directory, clock=self.clock)
        self.assertEqual(target.read_text(), "untouched")
        self.directory.mkdir()
        os.mkfifo(self.directory / "history.lock", 0o600)
        with self.assertRaises(RuntimeError):
            self.store()

    def test_hardlinked_database_and_symlink_directory_are_refused(self):
        target = Path(self.temporary.name) / "target"
        target.write_text("untouched")
        self.directory.mkdir()
        os.link(target, self.directory / "history.sqlite3")
        with self.assertRaises(RuntimeError):
            self.store()
        alias = Path(self.temporary.name) / "alias"
        alias.symlink_to(self.directory, target_is_directory=True)
        with self.assertRaises(RuntimeError):
            history.HistoryStore(alias, clock=self.clock)
        self.assertEqual(target.read_text(), "untouched")

    def test_missing_and_corrupt_history_are_errors_not_empty_results(self):
        self.directory.mkdir(mode=0o700)
        for operation in [lambda: self.records(), lambda: history.history_stats(self.directory)]:
            with self.assertRaises(FileNotFoundError):
                operation()
        self.assertEqual(list(self.directory.iterdir()), [])
        with self.store() as store:
            store.write(record(self.clock.now, 1))
            store.flush()
            store.connection.execute("UPDATE records SET payload=?", (b"not compressed",))
        with self.assertRaises(zlib.error):
            self.records()
        database = self.directory / history.DATABASE_NAME
        database.write_bytes(b"not a database")
        with self.assertRaises(sqlite3.DatabaseError):
            history.history_stats(self.directory)

    def test_sqlite_full_write_failure_is_not_swallowed(self):
        with self.store() as store:
            pages = store.connection.execute("PRAGMA page_count").fetchone()[0]
            store.connection.execute(f"PRAGMA max_page_count={pages}")
            with self.assertRaises(sqlite3.OperationalError):
                store.write(record(self.clock.now, 1, detail=random.Random(1).randbytes(65536).hex()))
            self.assertEqual(store.stats()["record_count"], 0)

    def test_read_only_write_failure_is_not_swallowed(self):
        with self.store() as store:
            store.connection.execute("PRAGMA query_only=ON")
            with self.assertRaises(sqlite3.OperationalError):
                store.write(record(self.clock.now, 1))
            self.assertEqual(store.stats()["record_count"], 0)
            store.connection.execute("PRAGMA query_only=OFF")

    def test_external_reader_pressure_blocks_growth_until_checkpoint_recovers(self):
        warning = io.StringIO()
        with contextlib.redirect_stderr(warning), self.store(max_bytes=256 * 1024) as store:
            store.write(record(self.clock.now - 100, -1))
            store.flush()
            reader = sqlite3.connect(self.directory / history.DATABASE_NAME)
            try:
                reader.execute("BEGIN")
                reader.execute("SELECT count(*) FROM records").fetchone()
                rng = random.Random(3)
                for identifier in range(100):
                    store.write(record(self.clock.now, identifier, detail=rng.randbytes(4096).hex()))
                with self.assertRaises(RuntimeError):
                    store.flush()
                self.assertTrue(store.stats()["over_budget"])
                with self.assertRaises(RuntimeError):
                    store.write(record(self.clock.now, 1000))
            finally:
                reader.close()
            self.assertFalse(store.maintain()["over_budget"])
            self.assertTrue(store.write(record(self.clock.now, 1000)))
        self.assertTrue(warning.getvalue())

    def test_clock_moving_backwards_commits_without_waiting_for_old_time(self):
        with self.store() as store:
            store.write(record(self.clock.now, 1))
            self.clock.now -= 60
            store.write(record(self.clock.now, 2))
            self.assertEqual(history.history_stats(self.directory)["record_count"], 2)


if __name__ == "__main__":
    unittest.main()
