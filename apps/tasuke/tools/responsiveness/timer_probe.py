"""What an app feels while a model runs: timer wakeup lateness (a 2 ms
period for 20 s, like a frame clock) and the wall time of a fixed CPU task
(sha256 over 8 x 64 MiB), five times. Prints one JSON line."""
import hashlib
import json
import sys
import time


def wakeups(seconds=20.0, period=0.002):
    late = []
    end = time.perf_counter() + seconds
    nxt = time.perf_counter() + period
    while time.perf_counter() < end:
        time.sleep(max(0.0, nxt - time.perf_counter()))
        now = time.perf_counter()
        late.append((now - nxt) * 1000)
        nxt = max(nxt + period, now + period / 2)
    late.sort()
    q = lambda f: round(late[min(len(late) - 1, int(f * len(late)))], 3)
    return {"n": len(late), "p50_ms": q(0.5), "p99_ms": q(0.99), "p999_ms": q(0.999), "max_ms": round(late[-1], 2),
            "over_8ms": sum(x > 8 for x in late)}


def task():
    data = b"x" * (64 << 20)
    t = time.perf_counter()
    for _ in range(8):
        hashlib.sha256(data).digest()
    return time.perf_counter() - t


runs = sorted(task() for _ in range(5))
print(json.dumps({"label": sys.argv[1], "wakeup": wakeups(), "task_s": {"median": round(runs[2], 3), "max": round(runs[-1], 3)}}))
