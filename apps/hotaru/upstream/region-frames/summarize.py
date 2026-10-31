#!/usr/bin/env python3
"""Reduce frozen remote frame trials; never launches the measured applications."""
import hashlib
import json
import shutil
import sys
import tarfile
from pathlib import Path

source, destination = map(Path, sys.argv[1:])
destination.mkdir(parents=True, exist_ok=True)
sha = lambda path: hashlib.sha256(path.read_bytes()).hexdigest()
report = json.loads((source / "attempt3/result.json").read_text())
assert report["status"] == "passed" and len(report["trials"]) == 6
summary = {key: report[key] for key in ["schema", "status", "recorded_utc", "scope", "roles", "topology", "sources", "controller_sha256"]}
summary["trials"] = []
for receipt in report["trials"]:
    name = f'{receipt["round"]:02}-{receipt["label"]}'
    folder = source / "attempt3" / name
    result = json.loads((folder / "run/result.json").read_text())
    assert receipt["accepted"] and receipt["returncode"] == 0
    assert not receipt["rejection_reasons"] and not receipt["compiler_overlap_samples"]
    assert sha(folder / "run/result.json") == receipt["result_sha256"]
    assert sha(folder / "activity.jsonl") == receipt["activity_sha256"]
    assert result["status"] == "passed"
    assert result["environment"]["AIZOME_NO_PIXEL_SCROLL"] == receipt["no_pixel_scroll"]
    hz = result["clock_ticks_per_second"]
    def counters(item):
        return {role: (item["after"][role]["cpu_ticks"] - before["cpu_ticks"]) * 1000 / hz
                for role, before in item["before"].items()}
    streams = []
    for stream in result["streams"]:
        entry = {key: value for key, value in stream.items() if key not in ["before", "after"]}
        entry["process_cpu_ms"] = counters(stream)
        entry["application_rss_after_kib"] = stream["after"]["application"]["memory_kib_except_threads"]["VmRSS"]
        streams.append(entry)
    summary["trials"].append({
        "label": receipt["label"], "round": receipt["round"], "accepted": True,
        "result_sha256": receipt["result_sha256"], "activity_sha256": receipt["activity_sha256"],
        "binary_sha256": result["binary_sha256"], "elf_sha256": result["running_executable_sha256"],
        "environment": result["environment"], "configuration": result["configuration"],
        "grid": result["grid"], "geometry": result["geometry"],
        "startup_ns": {key: value for key, value in result["startup"].items() if key.startswith("launch_to_")},
        "keyboard_count": len(result["keyboard"]), "keyboard_summary_ns": result["keyboard_summary_ns"],
        "controls": [{"active": control["active"], "duration_ns": control["end_ns"] - control["start_ns"],
                      "process_cpu_ms": counters(control), "captures": control["captures"],
                      "valid_captures": control["valid_captures"]} for control in result["observer_controls"]],
        "streams": streams,
    })
(destination / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
for name in ["compare.py", "tools/frame-benchmark.py", "tools/frame-matrix.py", "tools/frame-observer.c"]:
    target = destination / name
    target.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(source / "attempt3" / name, target)
with tarfile.open(destination / "raw.tar.gz", "w:gz") as archive:
    for path in sorted(source.rglob("*")):
        if not path.is_file() or path.is_symlink() or "__pycache__" in path.parts:
            continue
        # Preserve compiler identity/command receipt but not generated protocol
        # source or observer executables. The canonical observer source is above.
        if "observer-build" in path.parts and path.name != "build.json":
            continue
        if path.name.endswith(".lock"):
            continue
        archive.add(path, arcname=str(path.relative_to(source)), recursive=False)
manifest = {str(path.relative_to(destination)): {"sha256": sha(path), "bytes": path.stat().st_size}
            for path in sorted(destination.rglob("*")) if path.is_file() and path.name != "manifest.json"}
(destination / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
print(json.dumps({"trials": len(summary["trials"]), "raw_bytes": (destination / "raw.tar.gz").stat().st_size,
                  "raw_sha256": sha(destination / "raw.tar.gz")}))
