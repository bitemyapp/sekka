#!/usr/bin/env python3
"""Compare two built search examples, including positions and scanned bytes.

Run under the shared workstation lock. Both examples use legacy width and
resident history; parsing and result serialization are outside their timers.
This compares Rust candidates, not Ghostty or whole-application performance.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import random
import statistics
import subprocess
import tempfile


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline", type=Path, required=True)
    parser.add_argument("--candidate", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--cpu", type=int, default=8)
    args = parser.parse_args()
    os.sched_setaffinity(0, {args.cpu})
    binaries = {"baseline": args.baseline.resolve(), "candidate": args.candidate.resolve()}
    hashes = {name: digest(path) for name, path in binaries.items()}
    report = {"scope": "paired static Rust search; legacy width, resident history; not Ghostty parity",
              "binary_sha256": hashes, "driver_sha256": digest(Path(__file__)),
              "cpu": args.cpu, "load_before": os.getloadavg(), "warmup_rounds": 2,
              "rounds": 8, "searches_per_round": 20, "random_seed": 13199,
              "measurements": [], "random_equality_cases": 0}
    with tempfile.TemporaryDirectory(prefix="hotaru-search-compare-") as directory:
        stream = Path(directory) / "stream.vt"

        def run(engine, needle, iterations):
            return json.loads(subprocess.check_output(
                [str(binaries[engine]), str(stream), needle, str(iterations), "resident"], text=True))

        def equal(first, second):
            for field in ("total_matches", "matches", "bytes_scanned", "history_rows"):
                assert first[field] == second[field], field

        for lines in [1000, 10_000, 20_000]:
            stream.write_text("".join(f"{i:08} needle 雪 e\u0301 repeated text\r\n" for i in range(lines)))
            for needle in ["needle", "雪 e\u0301", "missing"]:
                samples = {name: [] for name in binaries}
                for round_index in range(-2, 8):
                    order = list(binaries) if round_index % 2 == 0 else list(reversed(binaries))
                    results = {name: run(name, needle, 20) for name in order}
                    equal(results["baseline"], results["candidate"])
                    if round_index >= 0:
                        for name in binaries:
                            samples[name].extend(results[name]["samples_ns"])
                medians = {name: statistics.median(values) for name, values in samples.items()}
                item = {"lines": lines, "needle": needle, "input_sha256": digest(stream),
                        "samples_ns": samples, "median_ns": medians,
                        "candidate_over_baseline": medians["candidate"] / medians["baseline"]}
                report["measurements"].append(item)
                print(lines, repr(needle), item["candidate_over_baseline"], flush=True)
        rng = random.Random(report["random_seed"])
        for _ in range(60):
            stream.write_text("".join(rng.choice([
                "A", "b", "c", " ", "\r\n", "雪", "a\u0301", "😀",
                "\x1b[31m", "\x1b[0m", "\r", "\x1b[2K", "\x1b[3G"])
                for _ in range(2000)))
            for needle in ["a", "ab", "aba", "aab", "bb", "AAA", "雪", "a\u0301", "\n", " \n", "\n\n", "😀c"]:
                equal(run("baseline", needle, 1), run("candidate", needle, 1))
                report["random_equality_cases"] += 1
    report["load_after"] = os.getloadavg()
    assert hashes == {name: digest(path) for name, path in binaries.items()}
    args.output.write_text(json.dumps(report, indent=2) + "\n")


if __name__ == "__main__":
    main()
