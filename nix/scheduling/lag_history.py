"""Private, compressed lag history, independent of the recent-detail JSONL ring.

Queries include both time boundaries and release their SQLite read transaction
before yielding each bounded batch. They are a live traversal, not a snapshot:
concurrent pruning can remove unread rows and backdated inserts before the cursor
are not revisited. Missing, unreadable and corrupt databases are errors.

The byte budget covers the greater of logical/allocated sizes of the database,
WAL and SHM. It is a best-effort post-maintenance bound, not a filesystem quota:
a write batch (up to 256 records / 1 MiB compressed, plus its last record), SQLite
page/index overhead and reclamation can transiently exceed it. Commits occur on
writes after five seconds, or at the batch thresholds, and on flush/close; an
idle writer needs flush() to make its pending batch visible. Maintenance runs at
most once a minute unless allocation pressure requires it sooner. A reader from
another program can pin WAL pages: unrecoverable over-budget allocation is
reported and prevents further appends rather than growing without bound.
"""

import datetime
import fcntl
import hashlib
import json
import math
import os
from pathlib import Path
import sqlite3
import stat
import sys
import time
import zlib


RETENTION_SECONDS = 72 * 3600
MAX_BYTES = 2 * 1024 ** 3
BATCH_RECORDS = 256
BATCH_BYTES = 1024 ** 2
COMMIT_SECONDS = 5
MAINTENANCE_SECONDS = 60
QUERY_BATCH_RECORDS = 128
DATABASE_NAME = "history.sqlite3"


def _existing_private_file(path, tighten=False):
    try:
        info = path.lstat()
    except FileNotFoundError:
        return False
    if (not stat.S_ISREG(info.st_mode) or info.st_uid != os.getuid()
            or info.st_nlink != 1):
        raise RuntimeError(f"unsafe history file: {path}")
    if tighten:
        path.chmod(0o600)
    elif stat.S_IMODE(info.st_mode) != 0o600:
        raise RuntimeError(f"history file must have mode 0600: {path}")
    return True


def _directory(output_dir, create=False):
    directory = Path(output_dir)
    if create:
        directory.mkdir(mode=0o700, parents=True, exist_ok=True)
    info = directory.lstat()
    if not stat.S_ISDIR(info.st_mode) or info.st_uid != os.getuid():
        raise RuntimeError("history directory must be a real directory owned by the current user")
    if create:
        directory.chmod(0o700)
    elif stat.S_IMODE(info.st_mode) != 0o700:
        raise RuntimeError("history directory must have mode 0700")
    return directory


def _check_files(directory, tighten=False):
    for suffix in ("", "-wal", "-shm", "-journal"):
        _existing_private_file(directory / (DATABASE_NAME + suffix), tighten)
    _existing_private_file(directory / "history.lock", tighten)


def _allocation(directory):
    sizes = {}
    for name, suffix in (("database_bytes", ""), ("wal_bytes", "-wal"),
                         ("shared_memory_bytes", "-shm")):
        path = directory / (DATABASE_NAME + suffix)
        try:
            info = path.lstat()
        except FileNotFoundError:
            sizes[name] = 0
            continue
        if not stat.S_ISREG(info.st_mode) or info.st_uid != os.getuid() or info.st_nlink != 1:
            raise RuntimeError(f"unsafe history file: {path}")
        sizes[name] = max(info.st_size, info.st_blocks * 512)
    sizes["allocated_bytes"] = sum(sizes.values())
    return sizes


def _json(value):
    return json.dumps(value, sort_keys=True, ensure_ascii=True,
                      separators=(",", ":"), allow_nan=False)


def _epoch(value):
    if isinstance(value, str):
        parsed = datetime.datetime.fromisoformat(value.replace("Z", "+00:00"))
        if parsed.tzinfo is None:
            raise ValueError("wall_utc must include a timezone")
        result = parsed.timestamp()
    elif isinstance(value, (int, float)) and not isinstance(value, bool):
        result = float(value)
    else:
        raise ValueError("wall_utc must be an ISO timestamp or epoch seconds")
    if not math.isfinite(result):
        raise ValueError("history timestamp must be finite")
    return result


def _identity(record, encoded):
    if record.get("run_id") is not None and record.get("sample_id") is not None:
        identity = _json([record["run_id"], record["type"], record["sample_id"]]).encode()
        return "sample:" + hashlib.sha256(identity).hexdigest()
    return "record:" + hashlib.sha256(encoded).hexdigest()


def _metadata(connection, key, default=None):
    row = connection.execute("SELECT value FROM metadata WHERE key = ?", (key,)).fetchone()
    return json.loads(row[0]) if row else default


def _set_metadata(connection, key, value):
    connection.execute("INSERT INTO metadata(key,value) VALUES (?,?) "
                       "ON CONFLICT(key) DO UPDATE SET value=excluded.value", (key, _json(value)))


def _stats(connection, directory):
    count = connection.execute("SELECT count(*) FROM records").fetchone()[0]
    oldest = connection.execute("SELECT wall FROM records ORDER BY wall,key LIMIT 1").fetchone()
    newest = connection.execute("SELECT wall FROM records ORDER BY wall DESC,key DESC LIMIT 1").fetchone()
    settings = _metadata(connection, "settings")
    if settings is None:
        raise RuntimeError("history database has no retention settings")
    result = {"record_count": count, "oldest_wall": oldest[0] if oldest else None,
              "newest_wall": newest[0] if newest else None, **settings,
              "retention_notice": _metadata(connection, "retention_notice"),
              **_allocation(directory)}
    result["over_budget"] = result["allocated_bytes"] > result["max_bytes"]
    return result


def _read_connection(output_dir):
    directory = _directory(output_dir)
    _check_files(directory)
    path = directory / DATABASE_NAME
    if not path.exists():
        raise FileNotFoundError(path)
    connection = sqlite3.connect(path.resolve().as_uri() + "?mode=ro", uri=True,
                                 isolation_level=None, timeout=5)
    try:
        if connection.execute("PRAGMA user_version").fetchone()[0] != 1:
            raise RuntimeError("unsupported history database schema")
        connection.execute("PRAGMA query_only=ON")
    except BaseException:
        connection.close()
        raise
    return directory, connection


def history_stats(output_dir):
    """Read committed statistics without acquiring the writer lock or creating a DB."""
    directory, connection = _read_connection(output_dir)
    try:
        return _stats(connection, directory)
    finally:
        connection.close()


def query_records(output_dir, since=None, until=None, kinds=None):
    """Yield records ordered by (wall timestamp, stable key), with inclusive bounds.

    No explicit/read transaction survives a fetch batch, even if the consumer
    stops iteration. The connection itself closes on exhaustion or generator.close().
    """
    since = _epoch(since) if since is not None else None
    until = _epoch(until) if until is not None else None
    if isinstance(kinds, str):
        kinds = [kinds]
    kinds = tuple(kinds) if kinds is not None else None
    directory, connection = _read_connection(output_dir)
    try:
        if kinds == ():
            return
        clauses, parameters = [], []
        if since is not None:
            clauses.append("wall >= ?")
            parameters.append(since)
        if until is not None:
            clauses.append("wall <= ?")
            parameters.append(until)
        if kinds is not None:
            clauses.append("kind IN (" + ",".join("?" for _ in kinds) + ")")
            parameters.extend(kinds)
        cursor_key = None
        while True:
            current_clauses, current_parameters = list(clauses), list(parameters)
            if cursor_key is not None:
                current_clauses.append("(wall,key) > (?,?)")
                current_parameters.extend(cursor_key)
            where = " WHERE " + " AND ".join(current_clauses) if current_clauses else ""
            cursor = connection.execute("SELECT wall,key,payload FROM records" + where
                                        + " ORDER BY wall,key LIMIT ?",
                                        [*current_parameters, QUERY_BATCH_RECORDS])
            rows = cursor.fetchall()
            cursor.close()
            if not rows:
                break
            cursor_key = rows[-1][:2]
            for _, _, payload in rows:
                record = json.loads(zlib.decompress(payload))
                if not isinstance(record, dict):
                    raise ValueError("history record is not an object")
                yield record
    finally:
        connection.close()


class HistoryStore:
    def __init__(self, output_dir, retention_seconds=RETENTION_SECONDS,
                 max_bytes=MAX_BYTES, clock=time.time):
        if not math.isfinite(retention_seconds) or retention_seconds <= 0:
            raise ValueError("retention_seconds must be positive and finite")
        if not isinstance(max_bytes, int) or isinstance(max_bytes, bool) or max_bytes <= 0:
            raise ValueError("max_bytes must be a positive integer")
        self.directory = _directory(output_dir, create=True)
        self.retention_seconds = retention_seconds
        self.max_bytes = max_bytes
        self.clock = clock
        self.connection = None
        self.lock = None
        self.pending_records = 0
        self.pending_bytes = 0
        self.last_commit = self.clock()
        self.last_maintenance = self.last_commit
        self.blocked = False
        try:
            lock_path = self.directory / "history.lock"
            _existing_private_file(lock_path, tighten=True)
            self.lock = os.open(lock_path, os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW, 0o600)
            info = os.fstat(self.lock)
            if not stat.S_ISREG(info.st_mode) or info.st_uid != os.getuid() or info.st_nlink != 1:
                raise RuntimeError("unsafe history lock file")
            os.fchmod(self.lock, 0o600)
            try:
                fcntl.flock(self.lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            except BlockingIOError as exc:
                raise RuntimeError("another history writer owns this output directory") from exc
            _check_files(self.directory, tighten=True)
            path = self.directory / DATABASE_NAME
            fd = os.open(path, os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW, 0o600)
            try:
                info = os.fstat(fd)
                if not stat.S_ISREG(info.st_mode) or info.st_uid != os.getuid() or info.st_nlink != 1:
                    raise RuntimeError("unsafe history database file")
                os.fchmod(fd, 0o600)
            finally:
                os.close(fd)
            self.connection = sqlite3.connect(path, isolation_level=None, timeout=0)
            version = self.connection.execute("PRAGMA user_version").fetchone()[0]
            if version not in (0, 1):
                raise RuntimeError("unsupported history database schema")
            if version == 0:
                tables = self.connection.execute("SELECT name FROM sqlite_master WHERE type='table'").fetchall()
                if tables:
                    raise RuntimeError("refusing an unrelated history database")
                self.connection.execute("PRAGMA auto_vacuum=INCREMENTAL")
            self.connection.execute("PRAGMA journal_mode=WAL")
            self.connection.execute("PRAGMA synchronous=NORMAL")
            self.connection.execute("PRAGMA wal_autocheckpoint=256")
            self.connection.execute(f"PRAGMA journal_size_limit={BATCH_BYTES}")
            self.connection.executescript(
                "BEGIN;"
                "CREATE TABLE IF NOT EXISTS records (key TEXT PRIMARY KEY, wall REAL NOT NULL,"
                "kind TEXT NOT NULL, payload BLOB NOT NULL);"
                "CREATE INDEX IF NOT EXISTS records_wall ON records(wall,key);"
                "CREATE TABLE IF NOT EXISTS metadata (key TEXT PRIMARY KEY,value TEXT NOT NULL);"
                "PRAGMA user_version=1;COMMIT;"
            )
            self.connection.execute("BEGIN")
            _set_metadata(self.connection, "settings", {"retention_seconds": retention_seconds,
                                                       "max_bytes": max_bytes})
            self.connection.commit()
            self.maintain(self.last_commit)
        except BaseException:
            self.close()
            raise

    def _require_open(self):
        if self.connection is None:
            raise RuntimeError("history store is closed")

    def write(self, record):
        """Queue a record; return False for an already-stored stable identity.

        The first version of a sample wins, so incident prehistory replay cannot
        multiply or overwrite an earlier full snapshot of that same sample.
        """
        self._require_open()
        if self.blocked:
            self.maintain()
            if self.blocked:
                raise RuntimeError("history allocation remains over budget; refusing further appends")
        if not isinstance(record, dict) or not isinstance(record.get("type"), str):
            raise ValueError("history records require a string type")
        wall = _epoch(record.get("wall_utc"))
        encoded = _json(record).encode()
        payload = zlib.compress(encoded, level=1)
        if not self.connection.in_transaction:
            self.connection.execute("BEGIN")
        cursor = self.connection.execute("INSERT OR IGNORE INTO records(key,wall,kind,payload) VALUES (?,?,?,?)",
                                         (_identity(record, encoded), wall, record["type"], payload))
        inserted = cursor.rowcount != 0
        self.pending_records += 1
        self.pending_bytes += len(payload) if inserted else 0
        now = self.clock()
        if (self.pending_records >= BATCH_RECORDS or self.pending_bytes >= BATCH_BYTES
                or now - self.last_commit >= COMMIT_SECONDS or now < self.last_commit):
            self.flush()
        if now - self.last_maintenance >= MAINTENANCE_SECONDS or now < self.last_maintenance:
            self.maintain(now)
        return inserted

    def _commit(self):
        self.connection.commit()
        self.pending_records = 0
        self.pending_bytes = 0
        self.last_commit = self.clock()

    def flush(self):
        self._require_open()
        self._commit()
        if _allocation(self.directory)["allocated_bytes"] > self.max_bytes:
            self.maintain()
            if self.blocked:
                raise RuntimeError("history allocation remains over budget after maintenance")

    def _reclaim(self):
        # Incremental vacuum moves pages without rebuilding/copying the whole DB.
        # Checkpoint between chunks so reclaim itself cannot build an enormous WAL.
        previous = None
        while True:
            free = self.connection.execute("PRAGMA freelist_count").fetchone()[0]
            if not free or free == previous:
                break
            previous = free
            self.connection.execute("PRAGMA incremental_vacuum(256)").fetchall()
            busy, _, _ = self.connection.execute("PRAGMA wal_checkpoint(TRUNCATE)").fetchone()
            if busy:
                break
        self.connection.execute("PRAGMA wal_checkpoint(TRUNCATE)").fetchone()

    def _live_bytes(self):
        page_count = self.connection.execute("PRAGMA page_count").fetchone()[0]
        free_count = self.connection.execute("PRAGMA freelist_count").fetchone()[0]
        page_size = self.connection.execute("PRAGMA page_size").fetchone()[0]
        return (page_count - free_count) * page_size

    def _delete(self, rows):
        self.connection.executemany("DELETE FROM records WHERE key=?", ((row[0],) for row in rows))

    def maintain(self, now=None):
        """Expire strictly older-than-window rows; retain the exact cutoff timestamp.

        Budget loss is durable metadata, not a rotating record. Its cumulative
        lost range survives restart and is never presented as full time coverage.
        """
        self._require_open()
        now = _epoch(self.clock() if now is None else now)
        self._commit()
        cutoff = now - self.retention_seconds
        while True:
            rows = self.connection.execute("SELECT key,wall FROM records WHERE wall < ? "
                                           "ORDER BY wall,key LIMIT ?", (cutoff, BATCH_RECORDS)).fetchall()
            if not rows:
                break
            self.connection.execute("BEGIN")
            self._delete(rows)
            self.connection.commit()
            self.connection.execute("PRAGMA wal_checkpoint(PASSIVE)").fetchone()
        self._reclaim()
        lost = 0
        sizes = _allocation(self.directory)
        if sizes["allocated_bytes"] > self.max_bytes:
            # Leave room for ordinary WAL/SHM growth and the notice itself.
            reserve = min(self.max_bytes // 8, 8 * 1024 ** 2)
            target = max(0, self.max_bytes - reserve - sizes["shared_memory_bytes"])
            while self._live_bytes() > target:
                rows = self.connection.execute("SELECT key,wall FROM records ORDER BY wall,key LIMIT 64").fetchall()
                if not rows:
                    break
                notice = _metadata(self.connection, "retention_notice", {})
                notice = {"reason": "byte_budget", "requested_retention_seconds": self.retention_seconds,
                          "first_detected_wall": notice.get("first_detected_wall", now),
                          "last_detected_wall": now,
                          "lost_since": min(notice.get("lost_since", rows[0][1]), rows[0][1]),
                          "lost_until": max(notice.get("lost_until", rows[-1][1]), rows[-1][1]),
                          "records_evicted": notice.get("records_evicted", 0) + len(rows)}
                self.connection.execute("BEGIN")
                self._delete(rows)
                _set_metadata(self.connection, "retention_notice", notice)
                self.connection.commit()
                lost += len(rows)
                self.connection.execute("PRAGMA wal_checkpoint(PASSIVE)").fetchone()
            self._reclaim()
        self.last_maintenance = now
        result = self.stats()
        self.blocked = result["over_budget"]
        if lost:
            print(f"sekka-lag-recorder: history byte budget evicted {lost} records inside "
                  f"the requested {self.retention_seconds:g}s retention window; "
                  f"lost range {result['retention_notice']['lost_since']}.."
                  f"{result['retention_notice']['lost_until']} (epoch seconds)", file=sys.stderr)
        if self.blocked:
            print(f"sekka-lag-recorder: history allocation {result['allocated_bytes']} exceeds "
                  f"byte budget {self.max_bytes}; checkpoint/reclamation could not enforce the cap",
                  file=sys.stderr)
        return result

    def stats(self):
        """Return current writer-visible counts, allocation and durable loss notice."""
        self._require_open()
        return _stats(self.connection, self.directory)

    def close(self):
        try:
            if self.connection is not None:
                try:
                    self.flush()
                    self.connection.execute("PRAGMA wal_checkpoint(TRUNCATE)").fetchone()
                finally:
                    self.connection.close()
                    self.connection = None
        finally:
            if self.lock is not None:
                os.close(self.lock)
                self.lock = None

    def __enter__(self):
        self._require_open()
        return self

    def __exit__(self, *args):
        self.close()
