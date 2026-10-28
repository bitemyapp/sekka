#!/usr/bin/env python3
"""Validate exact corpus state, then compare serial in-process feed timings.

The Zig and Rust adapters read input before timing and create fresh zero-history
terminals per iteration. This is a terminal-core workload comparison, never an
application-wide performance certification.
"""
import argparse
import datetime
import fcntl
import hashlib
import json
import os
from pathlib import Path
import platform
import statistics
import subprocess
import sys

from differential import Oracle, differences, digest
from corpus import PATTERNS


def state_hash(snapshot):
    common = {key: value for key, value in snapshot.items() if not key.startswith("_")}
    return hashlib.sha256(json.dumps(common, sort_keys=True, separators=(",", ":")).encode()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--library", required=True, type=Path)
    parser.add_argument("--reference", required=True, type=Path)
    parser.add_argument("--replay", required=True, type=Path)
    parser.add_argument("--revision", required=True)
    parser.add_argument("--candidate-metadata", required=True, type=Path)
    parser.add_argument("--data", required=True, type=Path)
    parser.add_argument("--report", required=True, type=Path)
    parser.add_argument("--baseline-metadata", type=Path,
                        default=Path(__file__).resolve().parent.parent / "upstream/baseline-build.json")
    parser.add_argument("--rows", type=int, default=80)
    parser.add_argument("--cols", type=int, default=120)
    parser.add_argument("--chunk", type=int, default=65536)
    parser.add_argument("--rounds", type=int, default=4)
    parser.add_argument("--iterations", type=int, default=5)
    parser.add_argument("--warmup", type=int, default=3)
    parser.add_argument("--cpu", type=int, help="single allowed CPU; defaults to first allowed CPU")
    parser.add_argument("--lock", type=Path, default=Path.home() / ".cache/hotaru-workstation.lock")
    args = parser.parse_args()
    if min(args.rows, args.cols, args.chunk, args.rounds, args.iterations, args.warmup) < 1:
        parser.error("dimensions, repetitions and warmups must be positive")
    if args.rows > 4096 or args.cols > 8192 or max(args.iterations, args.warmup) > 10000:
        parser.error("dimensions or iteration count exceeds replay bounds")
    args.lock.parent.mkdir(parents=True, exist_ok=True)
    args.report.parent.mkdir(parents=True, exist_ok=True)
    allowed_cpus = os.sched_getaffinity(0)
    cpu = min(allowed_cpus) if args.cpu is None else args.cpu
    if cpu not in allowed_cpus:
        parser.error("--cpu is outside this process's allowed affinity")
    os.sched_setaffinity(0, {cpu})
    metadata = json.loads(args.baseline_metadata.read_text())
    candidate_metadata = json.loads(args.candidate_metadata.read_text())
    if not candidate_metadata["build_confirmed"] or candidate_metadata["binary_sha256"] != digest(args.replay):
        parser.error("candidate metadata must identify the exact confirmed replay build")
    if metadata["revision"] != "6ba144158577d033ccb6ed4b574e4013957c9161" or not metadata["build_confirmed"]:
        parser.error("reference must be a confirmed pinned build")
    for key, path in (("libghostty-vt", args.library), ("ghostty-reference-replay", args.reference)):
        if metadata["binaries"][key]["sha256"] != digest(path):
            parser.error(f"reference binary hash mismatch: {key}")
    command_args = ["--data", str(args.data.resolve()), "--rows", str(args.rows),
                    "--cols", str(args.cols), "--chunk", str(args.chunk)]
    with args.lock.open("a") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        corpus_hash, candidate_hash = digest(args.data), digest(args.replay)
        data = args.data.read_bytes()
        oracle = Oracle(args.library)
        expected = oracle.snapshot(data, args.rows, args.cols, args.chunk)
        candidate = subprocess.run([str(args.replay.resolve()), *command_args, "--format", "json"],
                                   check=True, capture_output=True, text=True, timeout=60)
        actual = json.loads(candidate.stdout)
        delta = differences(expected, actual)
        # These features are absent from the current common snapshot and/or
        # Rust implementation. Equal visible text does not make their work
        # equivalent. Fail closed instead of celebrating discarded protocol.
        unsupported = []
        workload = next((name for name, pattern in PATTERNS.items()
                         if data and len(data) % len(pattern) == 0
                         and data == pattern * (len(data) // len(pattern))), None)
        if workload not in ("ascii", "styled", "unicode", "cursor"):
            unsupported.append("workload not in the reviewed protocol subset; visible snapshot parity alone is insufficient")
        for marker, feature in ((b"\x1b]8;", "OSC 8 hyperlink storage"),
                                (b"\x1b_G", "Kitty graphics"),
                                (b"\x1b]5522;", "Kitty clipboard"),
                                (b"\x1b[?2027h", "extended grapheme mode")):
            if marker in data:
                unsupported.append(feature)
        report = {
            "schema": 1, "recorded_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
            "upstream_revision": metadata["revision"], "candidate_revision": args.revision,
            "candidate_sha256": candidate_hash, "reference_sha256": digest(args.reference),
            "candidate_source_sha256": candidate_metadata["source_sha256"],
            "candidate_build_metadata_sha256": digest(args.candidate_metadata),
            "candidate_build_metadata": str(args.candidate_metadata.resolve()),
            "candidate_toolchain": candidate_metadata["toolchain"],
            "candidate_build_environment": candidate_metadata["environment"],
            "library_sha256": digest(args.library), "data": str(args.data.resolve()),
            "data_bytes": len(data), "data_sha256": corpus_hash,
            "rows": args.rows, "cols": args.cols, "scrollback": 0, "chunk_bytes": args.chunk,
            "reference_state_sha256": state_hash(expected), "candidate_state_sha256": state_hash(actual),
            "state_matched": not delta, "differences": delta,
            "unsupported_workload_features": unsupported,
            "reviewed_workload": workload,
            "performance_comparable": not delta and not unsupported,
            "scope": "in-process VT feed with zero history; identical compared snapshot precedes timing",
            "state_styles_compared": ["basic attributes", "underline variants", "overline"],
            "state_omissions": ["hyperlinks", "other modes/effects", "pending wrap", "graphics", "scrollback", "renderer"],
            "host": platform.node(), "platform": platform.platform(), "cpu_affinity": sorted(os.sched_getaffinity(0)),
            "load_average_before": os.getloadavg(), "warmup_iterations_per_engine": args.warmup,
            "timed_response_count_check": {},
        }
        if not delta and not unsupported:
            commands = {"ghostty": [str(args.reference.resolve()), *command_args, "--bench-json"],
                        "hotaru": [str(args.replay.resolve()), *command_args, "--bench-json"]}
            samples = {engine: {"samples_ns": [], "setup_ns": [], "teardown_ns": []} for engine in commands}

            def measure(engine, iterations):
                result = subprocess.run([*commands[engine], "--iterations", str(iterations)],
                                        check=True, capture_output=True, text=True, timeout=60)
                value = json.loads(result.stdout)
                for key, expected_value in (("schema", 1), ("engine", engine), ("timed_scope", "feed-only"),
                                            ("input_bytes", len(data)), ("rows", args.rows), ("cols", args.cols),
                                            ("scrollback", 0), ("chunk_bytes", args.chunk)):
                    if value[key] != expected_value:
                        raise RuntimeError(f"benchmark configuration mismatch: {engine}.{key}")
                for key in ("samples_ns", "setup_ns", "teardown_ns"):
                    if len(value[key]) != iterations or any(not isinstance(x, int) or x < 0 for x in value[key]):
                        raise RuntimeError(f"invalid samples: {engine}.{key}")
                if "response_bytes" in value:
                    if value["response_bytes"] != [len(expected["responses"])] * iterations:
                        raise RuntimeError(f"PTY responses were dropped or altered during timed feed: {engine}")
                    report["timed_response_count_check"][engine] = "each measured iteration"
                elif expected["responses"]:
                    raise RuntimeError(f"query workloads require timed response counts: {engine}")
                else:
                    # The four exact reviewed patterns above contain no reply
                    # commands; the oracle and candidate also verified zero
                    # replies. Do not invent a measured count for older replay
                    # binaries that lack this optional instrumentation.
                    report["timed_response_count_check"][engine] = "reviewed no-query pattern and pre-timing snapshot only"
                return value

            for engine in commands:
                measure(engine, args.warmup)
            order = []
            for iteration in range(args.rounds):
                for engine in (list(commands) if iteration % 2 == 0 else list(reversed(commands))):
                    batch = measure(engine, args.iterations)
                    order.append(engine)
                    for key in samples[engine]:
                        samples[engine][key].extend(batch[key])
            for engine, sample in samples.items():
                sample["median_feed_ns"] = statistics.median(sample["samples_ns"])
                sample["median_setup_ns"] = statistics.median(sample["setup_ns"])
                sample["median_teardown_ns"] = statistics.median(sample["teardown_ns"])
            report.update(commands=commands, process_order=order, measurements=samples,
                          hotaru_over_ghostty=samples["hotaru"]["median_feed_ns"] / samples["ghostty"]["median_feed_ns"])
        for path, original in ((args.data, corpus_hash), (args.replay, candidate_hash),
                               (args.reference, report["reference_sha256"]),
                               (args.library, report["library_sha256"]),
                               (args.candidate_metadata, report["candidate_build_metadata_sha256"])):
            if digest(path) != original:
                raise RuntimeError(f"measurement input changed during validation/measurement: {path}")
        report["load_average_after"] = os.getloadavg()
        args.report.write_text(json.dumps(report, indent=2) + "\n")
        print(json.dumps({"report": str(args.report), "state_matched": not delta,
                          "performance_comparable": not delta and not unsupported,
                          "hotaru_over_ghostty": report.get("hotaru_over_ghostty")}))
        if delta or unsupported:
            sys.exit(1)


if __name__ == "__main__":
    main()
