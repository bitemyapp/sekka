"""CAS/content-store extent sharing on the builder's filesystem (Btrfs to run)."""
import errno
import fcntl
import hashlib
import os
from pathlib import Path
import sys
import tempfile

sys.path.insert(0, str(Path(__file__).resolve().parent))
import cas_dedupe  # noqa: E402

FICLONE = 0x40049409


def reflinks(directory):
    with tempfile.TemporaryFile(dir=directory) as source, tempfile.TemporaryFile(dir=directory) as copy:
        source.write(b"probe" * 1024)
        source.flush()
        try:
            fcntl.ioctl(copy.fileno(), FICLONE, source.fileno())
        except OSError as error:
            if error.errno in (errno.EOPNOTSUPP, errno.ENOTTY, errno.EINVAL, errno.EXDEV):
                return False
            raise
    return True


def put(root, data, corrupt=False):
    digest = hashlib.sha256(data).hexdigest()
    (root / "store" / f"SHA256-{digest}-{len(data)}").write_bytes(data)
    blob = root / "cas/d2" / f"{digest}-{len(data)}-7"
    blob.write_bytes(data[:-1] + b"X" if corrupt else data)
    blob.chmod(0o444)
    return blob


def main():
    with tempfile.TemporaryDirectory() as temporary:
        root = Path(temporary)
        if not reflinks(root):
            print("CAS dedupe not checked: this builder's filesystem does not share extents.")
            return
        (root / "store").mkdir()
        (root / "cas/d2").mkdir(parents=True)
        shared = [put(root, os.urandom(size // 2) + bytes(size - size // 2))
                  for size in (40 * 2**20, 3 * 2**20 + 123, 70000)]
        put(root, os.urandom(100))
        mismatch = put(root, os.urandom(8192), corrupt=True)
        report = cas_dedupe.dedupe([root / "cas"], [root / "store"])
        assert report["candidates"] == 5, report
        assert report["deduplicated"] == 3 and report["skipped_small"] == 1, report
        assert report["differs"] == [str(mismatch)] and not report["errors"], report
        for blob in shared:
            data = blob.read_bytes()
            assert blob.name.startswith(hashlib.sha256(data).hexdigest()), blob
        mismatch.unlink()
        again = cas_dedupe.dedupe([root / "cas"], [root / "store"])
        assert again["already_shared"] == 3 and again["deduplicated"] == 0, again
        assert not again["differs"] and not again["errors"], again
    print("CAS dedupe: shared, mismatch and repeat checks passed")


if __name__ == "__main__":
    main()
