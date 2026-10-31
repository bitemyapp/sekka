#!/usr/bin/env python3
"""Measure current owned capture against the exact checkpoint's eager API.

Build the two documented drivers with native CPU, thin LTO, one codegen unit.
Run this under the shared workstation lock; each measurement is CPU-8 pinned.
"""
import argparse
import hashlib
import json
import pathlib
import platform
import subprocess
import time


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def sources(root):
    paths = sorted((root / "src").rglob("*.rs"))
    paths += sorted((root / "examples").glob("*.rs"))
    paths += [root / "Cargo.toml"]
    return {str(path.relative_to(root)): digest(path) for path in paths}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--current", type=pathlib.Path, required=True)
    parser.add_argument("--checkpoint", type=pathlib.Path, required=True)
    parser.add_argument("--current-source", type=pathlib.Path, required=True)
    parser.add_argument("--checkpoint-source", type=pathlib.Path, required=True)
    parser.add_argument("--output", type=pathlib.Path, required=True)
    args = parser.parse_args()
    report = {
        "captured_at_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "host": platform.node(),
        "cpu_affinity": 8,
        "columns": 80,
        "visible_rows": 24,
        "profile": "release, target-cpu=native, lto=thin, codegen-units=1",
        "allocator": "Rust system allocator; no trim or forced collection",
        "checkpoint_commit": "13423440ef68b3633b41d267aea196d87b1007da",
        "current_binary_sha256": digest(args.current),
        "checkpoint_binary_sha256": digest(args.checkpoint),
        "current_sources": sources(args.current_source),
        "checkpoint_sources": sources(args.checkpoint_source),
        "driver_sha256": digest(pathlib.Path(__file__)),
        "notes": [
            "Capture timings exclude release/destruction; release is measured separately.",
            "Owned capture has 500 repetitions/process; eager capture has 30.",
            "Each combination has five fresh processes, alternated current/checkpoint.",
            "Worker restore is one full iteration after terminal teardown; first-cell checksum checks consumption.",
            "No claim of constant-time capture: cost scales with block count plus visible rows.",
        ],
        "measurements": [],
    }
    for history in (1000, 10000, 100000):
        for storage in ("resident", "cold"):
            for sample in range(5):
                for engine in ("current", "checkpoint"):
                    binary = args.current if engine == "current" else args.checkpoint
                    command = ["taskset", "-c", "8", str(binary), str(history), storage]
                    if engine == "current":
                        command.append("owned")
                    result = json.loads(subprocess.check_output(command, text=True))
                    if engine == "current":
                        assert result["checksum"] == (history + 23) * ord("0") + ord(" ")
                    result.update(engine=engine, process_sample=sample)
                    report["measurements"].append(result)
                    print(json.dumps(result), flush=True)
    args.output.write_text(json.dumps(report, indent=2) + "\n")


if __name__ == "__main__":
    main()
