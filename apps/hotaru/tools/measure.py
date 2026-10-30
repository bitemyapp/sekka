#!/usr/bin/env python3
"""Serial process timing with reproducible inputs, host metadata and peak RSS.

Arguments after -- are executed directly, without a shell. This measures whole
process cost (including startup, file reads and teardown), not parser-only cost.
All cooperating builds/measurements must use the same --lock file.
"""
import argparse
import datetime
import fcntl
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import signal
import statistics
import subprocess
import tempfile
import threading
import time


def sha256(path):
    digest = hashlib.sha256()
    with Path(path).open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--label", required=True)
    parser.add_argument("--revision", required=True)
    parser.add_argument("--data", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--rows", type=int, default=80)
    parser.add_argument("--cols", type=int, default=120)
    parser.add_argument("--runs", type=int, default=15)
    parser.add_argument("--warmup", type=int, default=3)
    parser.add_argument("--timeout", type=float, default=120)
    parser.add_argument("--lock", type=Path, default=Path.home() / ".cache/hotaru-workstation.lock")
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    command = args.command[1:] if args.command[:1] == ["--"] else args.command
    if not command or min(args.rows, args.cols, args.runs) < 1 or args.warmup < 0 or args.timeout <= 0:
        parser.error("supply a command and positive dimensions, runs and timeout")
    command = [item.replace("{data}", str(args.data.resolve())).replace("{rows}", str(args.rows)).replace("{cols}", str(args.cols)) for item in command]
    executable = shutil.which(command[0])
    if not executable:
        parser.error(f"executable not found: {command[0]}")
    args.lock.parent.mkdir(parents=True, exist_ok=True)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    measurements = []
    with args.lock.open("a") as lock, tempfile.TemporaryDirectory(prefix="hotaru-timing-") as temporary:
        fcntl.flock(lock, fcntl.LOCK_EX)
        binary_hash = sha256(executable)
        corpus_hash = sha256(args.data)
        for index in range(args.warmup + args.runs):
            resource_file = Path(temporary) / "resources.json"
            wrapped = ["/usr/bin/time", "-f", '{"user_seconds":%U,"system_seconds":%S,"max_rss_kib":%M}', "-o", str(resource_file), "--", *command]
            started = time.perf_counter_ns()
            with (Path(temporary) / "stderr").open("wb") as error_log:
                process = subprocess.Popen(wrapped, stdout=subprocess.DEVNULL, stderr=error_log, start_new_session=True)
                expired = threading.Event()
                def timeout():
                    if process.poll() is None:
                        expired.set()
                        try:
                            os.killpg(process.pid, signal.SIGKILL)
                        except ProcessLookupError:
                            pass
                timer = threading.Timer(args.timeout, timeout)
                timer.daemon = True
                timer.start()
                try:
                    process.wait()
                finally:
                    timer.cancel()
                    timer.join()
                if expired.is_set():
                    raise SystemExit(f"benchmark exceeded {args.timeout} seconds; entire process group stopped")
            elapsed = (time.perf_counter_ns() - started) / 1e9
            if process.returncode:
                raise SystemExit(f"benchmark failed ({process.returncode}): " + (Path(temporary) / "stderr").read_text(errors="replace")[-8192:])
            if index >= args.warmup:
                sample = json.loads(resource_file.read_text())
                sample["wall_seconds"] = elapsed
                measurements.append(sample)
        if binary_hash != sha256(executable) or corpus_hash != sha256(args.data):
            raise SystemExit("binary or corpus changed during measurement; refusing evidence")
    cpuinfo = Path("/proc/cpuinfo").read_text()
    cpu_model = next((line.split(":", 1)[1].strip() for line in cpuinfo.splitlines() if line.startswith("model name")), "unknown")
    metadata = {
        "schema": 1, "recorded_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "label": args.label, "revision": args.revision, "command": command,
        "executable": str(Path(executable).resolve()), "executable_sha256": binary_hash,
        "data": str(args.data.resolve()), "data_sha256": corpus_hash, "data_bytes": args.data.stat().st_size,
        "rows": args.rows, "cols": args.cols, "warmup": args.warmup,
        "host": platform.node(), "platform": platform.platform(), "cpu_model": cpu_model,
        "cpu_affinity": sorted(os.sched_getaffinity(0)), "load_average_at_finish": os.getloadavg(),
        "samples": measurements, "median_wall_seconds": statistics.median(x["wall_seconds"] for x in measurements),
        "max_rss_kib": max(x["max_rss_kib"] for x in measurements),
        "scope": "whole-process timing only; correctness, GUI latency and complete feature parity are separate gates"
    }
    args.output.write_text(json.dumps(metadata, indent=2) + "\n")
    print(json.dumps({"output": str(args.output), "median_wall_seconds": metadata["median_wall_seconds"], "max_rss_kib": metadata["max_rss_kib"]}))


if __name__ == "__main__":
    main()
