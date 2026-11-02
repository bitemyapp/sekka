#!/usr/bin/env python3
"""Recompute the Markdown results table from a completed player_battery.py run."""

import argparse
import json
from pathlib import Path
import statistics

from player_battery import ensure_environment, estimate


def report(directory):
    manifest = json.loads((directory / "manifest.json").read_text())
    expected = ([(r["run"], r["player"]) for r in manifest["schedule"]] if "schedule" in manifest else
                [(i + 1, player) for i in range(len(manifest["starts"])) for player in manifest["players"]])
    rows = []
    for round_number, player in expected:
        path = directory / f"{round_number:02d}-{player}"
        if (path / "invalid.json").exists():
            raise ValueError(f"Invalid run: {path}")
        # after.json and summary.json are completion markers, never summarize a live partial run.
        after = json.loads((path / "after.json").read_text())
        recorded = json.loads((path / "summary.json").read_text())
        if manifest.get("measure_at_s") is not None and abs(recorded["measured_movie_start_s"] - manifest["measure_at_s"]) > 0.75:
            raise ValueError(f"Wrong movie interval: {path}")
        samples = [json.loads(line) for line in (path / "samples.jsonl").read_text().splitlines()]
        result = estimate(samples)
        if abs(result["seconds"] - manifest["measurement_s"]) > 2:
            raise ValueError(f"Wrong measurement duration: {path}")
        for s in samples:
            ensure_environment(s, manifest["brightness"], 0)
            if "playback" in s:
                w = s["playback"]["window"]
                if not w["fullscreen"] or not w["focused"]:
                    raise ValueError(f"Not fullscreen: {path}")
        if abs(after["position_s"] - recorded["measured_movie_start_s"] - result["seconds"]) > 3:
            raise ValueError(f"Playback did not advance normally: {path}")
        rows.append(dict(player=player, round=round_number, **result))
    range_label = "Range across runs (min)" if "schedule" in manifest else "Range across scenes (min)"
    lines = [f"| Player | Mean battery W | Full-charge minutes | Energy-slope check (min) | {range_label} | Minutes at common starting charge |",
             "| --- | ---: | ---: | ---: | ---: | ---: |"]
    initial_energy = manifest["battery"]["energy_wh"]
    for player in manifest["players"]:
        selected = [r for r in rows if r["player"] == player]
        seconds = sum(r["seconds"] for r in selected)
        power = sum(r["mean_power_w"] * r["seconds"] for r in selected) / seconds
        slope_power = sum(r["slope_power_w"] * r["seconds"] for r in selected) / seconds
        full = statistics.mean(r["full_wh"] for r in selected)
        projections = [r["projected_full_minutes"] for r in selected]
        lines.append(f"| {player} | {power:.3f} | {60 * full / power:.0f} | "
                     f"{60 * full / slope_power:.0f} | {min(projections):.0f}–{max(projections):.0f} | {60 * initial_energy / power:.0f} |")
    label = "runs" if "schedule" in manifest else "scenes"
    run_label = "Run" if "schedule" in manifest else "Round"
    lines += ["", f"Common starting energy: {initial_energy:.2f} Wh. Full-charge capacity: {full:.2f} Wh.",
              f"Ranges describe the observed {label}; they are not confidence or prediction intervals.", "",
              f"| {run_label} | Player | Seconds | Battery W | Energy endpoint W | Energy slope W | Energy used Wh | Full-charge minutes |",
              "| ---: | --- | ---: | ---: | ---: | ---: | ---: | ---: |"]
    for r in rows:
        lines.append(f"| {r['round']} | {r['player']} | {r['seconds']:.2f} | {r['mean_power_w']:.3f} | "
                     f"{r['endpoint_power_w']:.3f} | {r['slope_power_w']:.3f} | {r['energy_used_wh']:.3f} | "
                     f"{r['projected_full_minutes']:.0f} |")
    return "\n".join(lines) + "\n"


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    args = parser.parse_args()
    print(report(args.directory), end="")
