#!/usr/bin/env python3
"""Measure clonefile versus byte copies on an empty, disposable APFS volume.

Pass the mount point of a private disk image, never a host volume root. Creates
one probe directory and removes it on exit. Reports free-space deltas rather
than adding st_blocks (which double-counts APFS clones). Does not mount/format.
"""
import ctypes
import hashlib
import json
import os
from pathlib import Path
import shutil
import statistics
import sys
import tempfile
import time


def main():
    volume = Path(sys.argv[1]).resolve()
    if sys.platform != "darwin" or not volume.is_mount() or volume == Path("/"):
        raise SystemExit("Pass an already-mounted disposable APFS disk image")
    libc = ctypes.CDLL(None, use_errno=True)
    clone = libc.clonefile
    clone.argtypes = [ctypes.c_char_p, ctypes.c_char_p, ctypes.c_int]
    clone.restype = ctypes.c_int
    def free():
        os.sync()
        info = os.statvfs(volume)
        return info.f_bfree * info.f_frsize
    def digest(path):
        with path.open("rb") as stream:
            return hashlib.file_digest(stream, "sha256").hexdigest()
    with tempfile.TemporaryDirectory(prefix="sekka-cas-probe-", dir=volume) as temporary:
        root = Path(temporary)
        source = root / "source"
        with source.open("wb") as stream:
            for _ in range(64):
                stream.write(os.urandom(1024**2))
            stream.flush()
            os.fsync(stream.fileno())
        expected = digest(source)
        base_free = free()
        times = []
        for i in range(8):
            dest = root / f"clone-{i}"
            start = time.perf_counter()
            if clone(os.fsencode(source), os.fsencode(dest), 1) != 0:
                error = ctypes.get_errno()
                raise OSError(error, os.strerror(error))
            with dest.open("rb") as stream:
                os.fsync(stream.fileno())
            times.append(time.perf_counter() - start)
            assert dest.stat().st_ino != source.stat().st_ino
            assert dest.stat().st_nlink == source.stat().st_nlink == 1
            assert digest(dest) == expected
        cloned_free = free()
        copies = []
        for i in range(8):
            start = time.perf_counter()
            with source.open("rb") as src, (root / f"copy-{i}").open("wb") as dst:
                shutil.copyfileobj(src, dst, 1024**2)
                dst.flush()
                os.fsync(dst.fileno())
            copies.append(time.perf_counter() - start)
        copied_free = free()
        with (root / "clone-0").open("r+b") as stream:
            stream.write(b"X" * 1024**2)
            stream.flush()
            os.fsync(stream.fileno())
        changed_free = free()
        assert digest(source) == digest(root / "clone-1") == expected
        assert digest(root / "clone-0") != expected
        source.unlink()
        assert digest(root / "clone-1") == expected
        result = {"blob_bytes": 64 * 1024**2, "views": 8,
                  "logical_view_bytes": 512 * 1024**2,
                  "clone_allocation_delta_bytes": base_free - cloned_free,
                  "copy_allocation_delta_bytes": cloned_free - copied_free,
                  "one_mib_cow_allocation_delta_bytes": copied_free - changed_free,
                  "clone_with_fsync_seconds": times, "copy_with_fsync_seconds": copies,
                  "clone_with_fsync_median_seconds": statistics.median(times),
                  "copy_with_fsync_median_seconds": statistics.median(copies),
                  "checks": ["content equality", "distinct single-link inodes", "write isolation", "survives donor unlink"],
                  "timing_caveat": "both include file fsync; neither includes parent-directory fsync or F_FULLFSYNC"}
        print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
