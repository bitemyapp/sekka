#!/usr/bin/env python3
"""Build a frame-matrix calibration from one qualification run of each application.

Usage: frame-calibration.py HOTARU_RUN GHOSTTY_RUN OUTPUT

Each run is a tools/frame-benchmark.py --purpose qualification output. The
calibration qualifies only if both passed with the matched physical geometry,
on the same renderer, and, on a GPU renderer, Ghostty kept GPU presentation.
Calibration timings are not measurements.
"""
import json
from pathlib import Path
import shutil
import sys


def main():
    hotaru, ghostty, output = (Path(p).resolve() for p in sys.argv[1:4])
    runs = {"hotaru": hotaru, "ghostty": ghostty}
    results = {kind: json.loads((path / "result.json").read_text()) for kind, path in runs.items()}
    problems = []
    for kind, result in results.items():
        if result.get("kind") != kind or result.get("purpose") != "qualification":
            problems.append(f"{kind}: not a {kind} qualification run")
        if result.get("status") != "passed" or not result.get("physical_geometry_qualified"):
            problems.append(f"{kind}: did not pass with the matched physical geometry")
    renderers = {result.get("renderer", "software") for result in results.values()}
    if len(renderers) != 1:
        problems.append("the runs used different renderers")
    renderer = renderers.pop() if len(renderers) == 1 else None
    if renderer == "gpu" and not results["ghostty"].get("gpu", {}).get("ghostty", {}).get("gpu_presentation"):
        problems.append("ghostty fell back from GPU presentation")
    if results["hotaru"].get("fonts") != results["ghostty"].get("fonts"):
        problems.append("the runs used different fonts")
    if problems:
        print(json.dumps({"status": "not_qualified", "problems": problems}))
        return 1
    output.mkdir(parents=True, exist_ok=False)
    apps = {}
    for kind, result in results.items():
        run = f"{kind}-calibration"
        (output / run).mkdir()
        shutil.copy(runs[kind] / "result.json", output / run / "result.json")
        apps[kind] = {"run": run, "binary": result["commands"]["application"][3],
                      "runtime_executable": result["running_executable"],
                      "runtime_executable_sha256": result["running_executable_sha256"],
                      "window_pixels": result["window_pixels"], "grid": result["grid"],
                      "gpu": result.get("gpu", {}).get("application")}
    summary = {"scope": f"{renderer} calibration from one qualification run of each application; its timings are excluded",
               "status": "qualified_for_future_matched_measurements" if not problems else "not_qualified",
               "renderer": renderer, "problems": problems, "host": results["hotaru"].get("host"),
               "compositor": results["hotaru"].get("gpu", {}).get("compositor", {}).get("log"), "apps": apps}
    (output / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
    print(json.dumps({"status": summary["status"], "problems": problems, "output": str(output)}))
    return 0 if not problems else 1


if __name__ == "__main__":
    raise SystemExit(main())
