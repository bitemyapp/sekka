"""Share extents between a NativeLink filesystem CAS and Buck2 content stores.

On one host both keep every uploaded output: the content store reflinks it
into worktrees, and NativeLink's CAS serves it to the action cache. Objects
are matched by SHA-256 and size and shared with FIDEDUPERANGE, which compares
the bytes first, so a name never substitutes for content. Pairs whose first
extent is already shared are skipped: repeating costs two FIEMAP calls per
pair. Only extents change; both owners keep their names and their eviction.
"""
import concurrent.futures
import errno
import fcntl
import os
from pathlib import Path
import re
import stat
import struct

FIEMAP = 0xC020660B
FIDEDUPERANGE = 0xC0189436
STEP = 16 * 1024**2  # Btrfs dedupes at most 16 MiB per request.
BLOCK = 4096
# Pairs share independently; on wx-workstation, 8 threads shared real release
# outputs 2.5 times as fast as one, and more did not help.
WORKERS = min(8, os.cpu_count() or 1)
BLOB = re.compile(r"([0-9a-f]{64})-([0-9]+)(?:-([0-9]+))?")


def scan(content_dirs):
    """Every blob under NativeLink content paths, as (SHA-256, size, generation, path).

    One pass over directory entries: no stat calls and no Path objects, which
    cost seconds at the CAS's maximum of about 500,000 blobs."""
    blobs = []
    for content in content_dirs:
        try:
            folders = [folder.path for folder in os.scandir(content) if folder.is_dir(follow_symlinks=False)]
        except FileNotFoundError:
            continue
        for folder in folders:
            with os.scandir(folder) as entries:
                for entry in entries:
                    match = BLOB.fullmatch(entry.name)
                    if match:
                        blobs.append((match[1], int(match[2]), int(match[3] or 0), entry.path))
    return blobs


def first_extent(fd):
    """Physical address of a file's first data extent, or None when inline/unmapped."""
    request = bytearray(32 + 56)
    struct.pack_into("=QQIII", request, 0, 0, 2**64 - 1, 0, 0, 1)
    fcntl.ioctl(fd, FIEMAP, request, True)
    mapped = struct.unpack_from("=I", request, 20)[0]
    physical, flags = struct.unpack_from("=Q", request, 40)[0], struct.unpack_from("=I", request, 72)[0]
    return physical if mapped and not flags & 0x306 else None  # UNKNOWN, DELALLOC, NOT_ALIGNED, INLINE


def store_objects(stores):
    objects = {}
    for store in stores:
        for entry in os.scandir(store):
            match = re.fullmatch(r"SHA256-([0-9a-f]{64})-([0-9]+)", entry.name)
            if match and entry.is_file(follow_symlinks=False):
                objects.setdefault((match[1], int(match[2])), entry.path)
    return objects


class Changed(Exception):
    """A file's size or type no longer matches its digest."""


def share_all(pairs):
    """share() over (source, blob, size) triples on WORKERS threads, in order.

    Each result is share()'s outcome or the OSError or Changed it raised."""
    def attempt(pair):
        try:
            return share(*pair)
        except (OSError, Changed) as error:
            return error
    with concurrent.futures.ThreadPoolExecutor(WORKERS) as pool:
        return list(pool.map(attempt, pairs))


def share(source, blob, size):
    """Share `blob`'s extents with `source`. Returns 'shared', 'already' or 'differs'."""
    src = os.open(source, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC)
    try:
        dst = os.open(blob, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC)
        try:
            for fd in (src, dst):
                status = os.fstat(fd)
                if status.st_size != size or not stat.S_ISREG(status.st_mode):
                    raise Changed()
            start = first_extent(src)
            if start is not None and start == first_extent(dst):
                return "already"
            offset = 0
            while offset < size:
                request = bytearray(24 + 32)
                struct.pack_into("=QQHHI", request, 0, offset, min(STEP, size - offset), 1, 0, 0)
                struct.pack_into("=qQQiI", request, 24, dst, offset, 0, 0, 0)
                fcntl.ioctl(src, FIDEDUPERANGE, request, True)
                done, status = struct.unpack_from("=Qi", request, 40)
                if status == 1:
                    return "differs"
                if status < 0:
                    raise OSError(-status, os.strerror(-status))
                if done == 0:
                    raise OSError(errno.EIO, "kernel shared no bytes")
                offset += done
            return "shared"
        finally:
            os.close(dst)
    finally:
        os.close(src)


def dedupe(content_dirs, stores, blobs=None):
    """Share every persistent CAS blob's extents with an identical store object.

    content_dirs are NativeLink filesystem-store content paths, whose blobs are
    named HASH-SIZE in subdirectories (with a generation suffix in layout v2).
    `blobs` is a callable returning their scan, to share one scan between passes.
    A cross-filesystem pair stops the pass instead of failing every blob."""
    objects = store_objects(stores)
    report = {"schema": 1, "content_dirs": [str(d) for d in content_dirs],
              "content_stores": [str(s) for s in stores], "candidates": 0, "already_shared": 0,
              "deduplicated": 0, "deduplicated_bytes": 0, "skipped_small": 0,
              "cross_filesystem": False, "differs": [], "errors": []}
    if not objects:
        return report
    work = []
    for digest, size, _, blob in (blobs or (lambda: scan(content_dirs)))():
        source = objects.get((digest, size))
        if not source:
            continue
        report["candidates"] += 1
        if size < BLOCK:
            report["skipped_small"] += 1  # Inline or a single partial block: nothing to share.
            continue
        work.append((source, blob, size))
    for (_, blob, size), outcome in zip(work, share_all(work)):
        if isinstance(outcome, FileNotFoundError):
            continue  # Evicted by NativeLink or the content store meanwhile.
        if isinstance(outcome, Changed):
            report["errors"].append({"blob": blob, "error": "size or type differs from its digest name"})
            continue
        if isinstance(outcome, OSError):
            if outcome.errno == errno.EXDEV:
                report["cross_filesystem"] = True
                return report
            report["errors"].append({"blob": blob, "error": str(outcome)})
            continue
        if outcome == "differs":
            report["differs"].append(blob)
        elif outcome == "already":
            report["already_shared"] += 1
        else:
            report["deduplicated"] += 1
            report["deduplicated_bytes"] += size
    return report


def cas_index(blobs):
    """(SHA-256, size) to the newest-generation blob file, from a scan."""
    index = {}
    for digest, size, generation, blob in blobs:
        key = (digest, size)
        if key not in index or generation > index[key][0]:
            index[key] = (generation, blob)
    return {key: blob for key, (_, blob) in index.items()}


def share_journal(content_dirs, journal, blobs=None):
    """Share extents of journaled outputs (HASH SIZE PATH lines) with their CAS blobs.

    The engine lists every produced output it found in or put into the CAS. The
    journal is renamed first, so new entries start a new file. An output that
    changed or vanished since it was listed is skipped, not an error. One whose
    blob is not visible yet is kept in JOURNAL.retry for exactly one more pass.
    `blobs` is as for dedupe()."""
    report = {"schema": 1, "entries": 0, "already_shared": 0, "deduplicated": 0, "deduplicated_bytes": 0,
              "skipped_small": 0, "missing_blob": 0, "changed": 0, "cross_filesystem": False, "errors": []}
    pending = Path(str(journal) + ".sharing")
    retry = Path(str(journal) + ".retry")
    try:
        os.rename(journal, pending)
    except FileNotFoundError:
        if not pending.exists() and not retry.exists():
            return report  # Nothing listed; a leftover .sharing file is retried.
    entries = {}
    retried = set()
    for source, again in ((retry, True), (pending, False)):
        if source.exists():
            for line in source.read_text().splitlines():
                parts = line.split(" ", 2)
                if len(parts) == 3 and parts[1].isdigit():
                    entries[parts[2]] = (parts[0], int(parts[1]))  # The latest listing of a path wins.
                    (retried.add if again else retried.discard)(parts[2])
    later = []
    index = None
    work = []
    for path, (digest, size) in entries.items():
        report["entries"] += 1
        if size < BLOCK:
            # Includes empty outputs, whose blob a CAS never stores.
            report["skipped_small"] += 1
            continue
        if index is None:
            index = cas_index((blobs or (lambda: scan(content_dirs)))())
        blob = index.get((digest, size))
        if blob is None:
            report["missing_blob"] += 1
            if path not in retried:
                later.append(f"{digest} {size} {path}\n")
            continue
        work.append((blob, path, size))
    for (_, path, size), outcome in zip(work, share_all(work)):
        if isinstance(outcome, (FileNotFoundError, Changed)):
            report["changed"] += 1
            continue
        if isinstance(outcome, OSError):
            if outcome.errno == errno.EXDEV:
                report["cross_filesystem"] = True
                break
            report["errors"].append({"path": path, "error": str(outcome)})
            continue
        if outcome == "differs":
            report["changed"] += 1  # Rewritten in place since it was listed.
        elif outcome == "already":
            report["already_shared"] += 1
        else:
            report["deduplicated"] += 1
            report["deduplicated_bytes"] += size
    if later:
        temporary = Path(str(retry) + ".new")
        temporary.write_text("".join(later))
        os.replace(temporary, retry)
    else:
        retry.unlink(missing_ok=True)
    pending.unlink(missing_ok=True)
    return report
