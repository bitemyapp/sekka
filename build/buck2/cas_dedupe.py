"""Share extents between a NativeLink filesystem CAS and Buck2 content stores.

On one host both keep every uploaded output: the content store reflinks it
into worktrees, and NativeLink's CAS serves it to the action cache. Objects
are matched by SHA-256 and size and shared with FIDEDUPERANGE, which compares
the bytes first, so a name never substitutes for content. Pairs whose first
extent is already shared are skipped: repeating costs two FIEMAP calls per
pair. Only extents change; both owners keep their names and their eviction.
"""
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


def share(source, blob, size):
    """Share `blob`'s extents with `source`. Returns 'shared', 'already' or 'differs'."""
    src = os.open(source, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC)
    try:
        dst = os.open(blob, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC)
        try:
            for fd in (src, dst):
                status = os.fstat(fd)
                if status.st_size != size or not stat.S_ISREG(status.st_mode):
                    raise OSError(errno.EINVAL, "size or type differs from its digest name")
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


def dedupe(content_dirs, stores):
    """Share every persistent CAS blob's extents with an identical store object.

    content_dirs are NativeLink filesystem-store content paths, whose blobs are
    named HASH-SIZE in subdirectories (with a generation suffix in layout v2).
    A cross-filesystem pair stops the pass instead of failing every blob."""
    objects = store_objects(stores)
    report = {"schema": 1, "content_dirs": [str(d) for d in content_dirs],
              "content_stores": [str(s) for s in stores], "candidates": 0, "already_shared": 0,
              "deduplicated": 0, "deduplicated_bytes": 0, "skipped_small": 0,
              "cross_filesystem": False, "differs": [], "errors": []}
    for content in content_dirs:
        for blob in Path(content).glob("*/*"):
            match = re.fullmatch(r"([0-9a-f]{64})-([0-9]+)(-[0-9]+)?", blob.name)
            source = match and objects.get((match[1], int(match[2])))
            if not source:
                continue
            size = int(match[2])
            report["candidates"] += 1
            if size < BLOCK:
                report["skipped_small"] += 1  # Inline or a single partial block: nothing to share.
                continue
            try:
                outcome = share(source, blob, size)
            except FileNotFoundError:
                continue  # Evicted by NativeLink or the content store meanwhile.
            except OSError as error:
                if error.errno == errno.EXDEV:
                    report["cross_filesystem"] = True
                    return report
                report["errors"].append({"blob": str(blob), "error": str(error)})
                continue
            if outcome == "differs":
                report["differs"].append(str(blob))
            elif outcome == "already":
                report["already_shared"] += 1
            else:
                report["deduplicated"] += 1
                report["deduplicated_bytes"] += size
    return report
