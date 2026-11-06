"""A desktop busy with its own work: 16 processes (every hardware thread on
the 9800X3D) hashing 64 MiB blocks for 20 s; prints the blocks done and the
window, as one JSON line."""
import hashlib
import json
import multiprocessing
import sys
import time

SECONDS = 20.0


def work(_):
    data = b"x" * (64 << 20)
    end = time.time() + SECONDS
    n = 0
    while time.time() < end:
        hashlib.sha256(data).digest()
        n += 1
    return n


if __name__ == "__main__":
    start = time.time()
    with multiprocessing.Pool(16) as pool:
        blocks = sum(pool.map(work, range(16)))
    print(json.dumps({"label": sys.argv[1], "blocks": blocks, "start": round(start, 1), "end": round(time.time(), 1)}))
