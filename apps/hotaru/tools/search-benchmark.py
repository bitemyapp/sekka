#!/usr/bin/env python3
"""Measure whole static search on the same pinned CPU, verifying every match.

Run under the shared workstation lock. Build the Rust search example with native
CPU, thin LTO and one codegen unit after cleaning its package. Reference search
creation/set/feed/run and Rust capture/scan are timed; parsing, result reads and
destruction are outside both timed regions. This is not a live-search benchmark.
"""
import argparse
import ctypes as C
import importlib.util
import json
import os
from pathlib import Path
import platform
import statistics
import subprocess
import tempfile
import time

from differential import checked, digest

spec = importlib.util.spec_from_file_location("search_oracle", Path(__file__).with_name("search-oracle.py"))
reference = importlib.util.module_from_spec(spec)
spec.loader.exec_module(reference)


def ghostty(oracle, path, needle, iterations):
    lib, terminal = oracle.lib, C.c_void_p()
    checked(lib.ghostty_terminal_new(None, C.byref(terminal), 80, 24))
    try:
        history_bytes = C.c_size_t(50_000_000)
        checked(lib.ghostty_terminal_set(terminal, oracle.enum("GhosttyTerminalOption", "SCROLLBACK_MAX_BYTES"), C.byref(history_bytes)))
        data = path.read_bytes()
        for at in range(0, len(data), 65_536):
            chunk = data[at:at + 65_536]
            lib.ghostty_terminal_vt_write(terminal, chunk, len(chunk))
        samples, matches = [], None
        raw = C.create_string_buffer(needle.encode())
        string = reference.String(C.cast(raw, C.c_void_p), len(needle.encode()))
        for _ in range(iterations):
            search = C.c_void_p()
            start = time.perf_counter_ns()
            checked(lib.ghostty_search_new(None, C.byref(search), terminal))
            try:
                checked(lib.ghostty_search_set(search, oracle.enum("GhosttySearchOption", "NEEDLE"), C.byref(string)))
                checked(lib.ghostty_search_run(search))
                samples.append(time.perf_counter_ns() - start)
                count = oracle.query("ghostty_search_get", search, "GhosttySearchData", "TOTAL_MATCHES", C.c_size_t)
                if matches is None:
                    matches = reference.selections(oracle, search, terminal, exclusive=True)
                assert count == len(matches)
            finally:
                lib.ghostty_search_free(search)
        history_rows = oracle.query("ghostty_terminal_get", terminal, "GhosttyTerminalData", "SCROLLBACK_ROWS", C.c_size_t)
        return {"samples_ns": samples, "matches": matches, "total_matches": len(matches), "history_rows": history_rows}
    finally:
        lib.ghostty_terminal_free(terminal)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--library", required=True, type=Path)
    parser.add_argument("--candidate", required=True, type=Path)
    parser.add_argument("--source", required=True, type=Path, help="Hotaru Cargo workspace")
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--cpu", type=int, default=8)
    parser.add_argument("--iterations", type=int, default=20)
    args = parser.parse_args()
    os.sched_setaffinity(0, {args.cpu})
    oracle, expected = reference.open_oracle(args.library)
    report = {"schema": 1, "recorded_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
              "host": platform.node(), "cpu": args.cpu, "columns": 80, "active_rows": 24,
              "load_before": os.getloadavg(), "upstream_revision": expected["revision"],
              "rustc": subprocess.check_output(["rustc", "+1.95.0", "-Vv"], text=True).strip(),
              "cargo_lock_sha256": digest(args.source / "Cargo.lock"),
              "library_sha256": digest(args.library), "candidate_sha256": digest(args.candidate),
              "profile": "release target-cpu=native thin-LTO codegen-units=1 System allocator",
              "driver_sha256": digest(Path(__file__)), "oracle_driver_sha256": digest(Path(reference.__file__)),
              "candidate_sources": {str(p.relative_to(args.source)): digest(p) for p in sorted((args.source / "crates/hotaru-core").rglob("*.rs"))},
              "notes": ["Resident history only; no compressed-history or live-update comparison.",
                        "Reference C API byte cap explicitly set to application default 50,000,000; row counts checked equal.",
                        "Every selected coordinate is compared, after inclusive-to-exclusive cell conversion.",
                        "Both include capture/feeding; both exclude terminal setup, result reads and release.",
                        "Each engine performs repeated fresh search construction on the same unchanged terminal.",
                        "C API timing includes ctypes dispatch; this matters most for very short searches.",
                        "CPU affinity and effort lock do not prevent unrelated machine load."], "measurements": []}
    with tempfile.TemporaryDirectory(prefix="hotaru-search-benchmark-") as directory:
        path = Path(directory) / "input.vt"
        for lines in [1000, 10_000, 20_000]:
            path.write_bytes("".join(f"{i:08} needle 雪 e\u0301 repeated text\r\n" for i in range(lines)).encode())
            for index, needle in enumerate(["needle", "雪 e\u0301", "missing"]):
                results = {}
                for engine in (["ghostty", "hotaru"] if index % 2 == 0 else ["hotaru", "ghostty"]):
                    if engine == "ghostty":
                        results[engine] = ghostty(oracle, path, needle, args.iterations)
                    else:
                        results[engine] = json.loads(subprocess.check_output([str(args.candidate.resolve()), str(path), needle, str(args.iterations), "resident"], text=True))
                        assert results[engine]["history_rows"] == lines + 1 - 24, "candidate history truncated"
                assert results["ghostty"]["history_rows"] == results["hotaru"]["history_rows"], "unequal retained history"
                assert results["ghostty"]["matches"] == results["hotaru"]["matches"], (lines, needle)
                for result in results.values():
                    result.pop("matches")
                    result["median_ns"] = statistics.median(result["samples_ns"])
                item = {"lines": lines, "needle": needle, "input_sha256": digest(path), **results,
                        "rust_over_ghostty": results["hotaru"]["median_ns"] / results["ghostty"]["median_ns"]}
                report["measurements"].append(item)
                print(json.dumps({"lines":lines,"needle":needle,"rust_over_ghostty":item["rust_over_ghostty"]}), flush=True)
    report["load_after"] = os.getloadavg()
    args.output.write_text(json.dumps(report, indent=2, ensure_ascii=False) + "\n")


if __name__ == "__main__":
    main()
