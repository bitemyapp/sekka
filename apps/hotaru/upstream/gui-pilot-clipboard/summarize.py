#!/usr/bin/env python3
"""Summarize accepted pilot observations; preserve every raw result separately.

Run beside the extracted `startup/` and `steady/` matrix directories.
Adapted from `../gui-pilot-sprites/summarize.py` for several steady trials.
"""
from pathlib import Path
import json
import statistics

root = Path(__file__).resolve().parent
summary = {
    "schema": 1,
    "status": "passed",
    "scope": "four alternating startup pairs and two sustained pairs in private Sway/pixman; pilot only",
    "product_commit": "2f2f73e0",
    "startup": {},
    "steady": {},
    "measurement_gates": {},
    "limits": [
        "No physical scanout or hardware-rendered app comparison",
        "Ghostty uses Mesa llvmpipe; Hotaru uses native CPU rasterization and SHM",
        "Fresh isolated application configuration/cache per trial; kernel page cache is not cleared",
        "Two sustained pairs cannot establish statistical parity or a causal optimization effect",
        "Captured sequence/CRC markers are not a checksum of all text pixels",
        "Process RSS in this zero-history GUI fixture does not measure retained-history efficiency",
        "Ghostty's calibration is the earlier sprite pilot's, for the unchanged binary",
    ],
}


def ms(ns):
    return ns / 1_000_000


def distribution(values):
    return {"samples": values, "median": statistics.median(values), "min": min(values), "max": max(values)}


def resources(before, after, ticks):
    return {
        role: {
            "cpu_ms": (after[role]["cpu_ticks"] - before[role]["cpu_ticks"]) / ticks * 1000,
            "rss_kib_before": before[role]["memory_kib_except_threads"]["VmRSS"],
            "rss_kib_after": after[role]["memory_kib_except_threads"]["VmRSS"],
        }
        for role in ("application", "compositor", "observer", "fixture")
    }


for phase in ("startup", "steady"):
    directory = root / phase
    matrix = json.loads((directory / "result.json").read_text())
    assert matrix["status"] == "passed"
    assert all(t["accepted"] and not t["rejection_reasons"] for t in matrix["trials"])
    summary["measurement_gates"][phase] = {
        "trials": len(matrix["trials"]),
        "cpu_sets": matrix["cpu_sets"],
        "compiler_overlap_samples": sum(t["compiler_overlap_samples"] for t in matrix["trials"]),
        "matrix_sources": matrix["sources"],
    }
    for kind in ("hotaru", "ghostty"):
        results = [json.loads(p.read_text()) for p in sorted(directory.glob("*-" + kind + "/run/result.json"))]
        identities = {(d["running_executable"], d["running_executable_sha256"], d["binary_sha256"]) for d in results}
        assert len(identities) == 1
        executable, elf, wrapper = identities.pop()
        result = {"executable": executable, "elf_sha256": elf, "wrapper_sha256": wrapper}
        if phase == "startup":
            for name in ("launch_to_pty_ready_ns", "launch_to_matching_capture_ns"):
                result[name.replace("_ns", "_ms")] = distribution([ms(d["startup"][name]) for d in results])
        else:
            result["trials"] = []
            for d in results:
                ticks = d["clock_ticks_per_second"]
                result["trials"].append({
                    "keyboard_samples": len(d["keyboard"]),
                    "keyboard_injection_to_capture_ms": {k: ms(v) for k, v in d["keyboard_summary_ns"].items()},
                    "static_controls": [
                        {"observer_active": c["active"], "duration_seconds": (c["end_ns"] - c["start_ns"]) / 1e9,
                         "captures": c["captures"], "resources": resources(c["before"], c["after"], ticks)}
                        for c in d["observer_controls"]
                    ],
                    "streams": [
                        {"kind": s["kind"], "producer_hz": s["producer_hz"], "produced": s["produced"],
                         "observed_unique": s["observed_unique"], "unobserved_or_coalesced": s["unobserved_or_coalesced"],
                         "final_write_to_capture_ms": ms(s["final_write_to_capture_ns"]),
                         "observed_unique_per_second": s["observed_unique_per_second"],
                         "resources": resources(s["before"], s["after"], ticks)} for s in d["streams"]
                    ],
                })
        summary[phase][kind] = result

(root / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
print(json.dumps({
    "startup_capture_ms": {k: v["launch_to_matching_capture_ms"]["median"] for k, v in summary["startup"].items()},
    "keyboard_p50_ms": {k: [t["keyboard_injection_to_capture_ms"]["p50"] for t in v["trials"]]
                        for k, v in summary["steady"].items()},
}, indent=2))
