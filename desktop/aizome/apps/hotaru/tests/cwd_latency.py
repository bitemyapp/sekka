#!/usr/bin/env python3
"""Alternate two cwd_latency example binaries on the local host; retain all samples.

Run inside the root flake's Aizome development shell (fonts/shared libraries).
Each binary runs real interactive shells using the native headless application
harness. This does not measure compositor presentation or physical input latency.
"""

import argparse
from datetime import datetime, timezone
import hashlib
import json
import lzma
import os
from pathlib import Path
import platform
import random
import statistics
import subprocess


def percentile(values, fraction):
    ordered = sorted(values)
    return ordered[round((len(ordered) - 1) * fraction)]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("baseline", type=Path)
    parser.add_argument("candidate", type=Path)
    parser.add_argument("resources", type=Path)
    parser.add_argument("evidence", type=Path)
    parser.add_argument("--bash", required=True)
    parser.add_argument("--fish", required=True)
    parser.add_argument("--blocks", type=int, default=8)
    args = parser.parse_args()
    started = datetime.now(timezone.utc).isoformat()
    load_before = os.getloadavg()
    args.evidence.mkdir(parents=True, exist_ok=True)
    binaries = {"A": args.baseline.resolve(), "B": args.candidate.resolve()}
    scenarios = ["bash-reports", "bash-no-reports", "fish-reports"]
    samples = []

    def run(variant, scenario, label):
        target = args.evidence / f"{label}-{scenario}-{variant}.json"
        shell = args.fish if scenario.startswith("fish") else args.bash
        subprocess.run(
            [str(binaries[variant]), scenario, shell, str(args.resources.resolve()), str(target)],
            check=True, timeout=30,
        )
        sample = json.loads(target.read_text())
        target.unlink()
        return sample

    # Warm the executable/font/filesystem caches for each combination; each
    # measured invocation still starts a fresh application process and shell.
    for scenario in scenarios:
        for variant in "AB":
            run(variant, scenario, "warmup")
    for block in range(args.blocks):
        order = "ABBA" if block % 2 == 0 else "BAAB"
        rotation = block % len(scenarios)
        for scenario in scenarios[rotation:] + scenarios[:rotation]:
            for position, variant in enumerate(order):
                sample = run(variant, scenario, f"{block}-{position}")
                sample.update(block=block, position=position, variant=variant)
                samples.append(sample)
        print(f"Completed block {block + 1}/{args.blocks}: {order}", flush=True)

    metrics = ["startup_us", "startup_shaped_us", "commands_us", "commands_shaped_us", "commands_cpu_us", "palette_us"]
    rng = random.Random(250)
    summary = {}
    for scenario in scenarios:
        rows = [sample for sample in samples if sample["scenario"] == scenario]
        summary[scenario] = {}
        for metric in metrics:
            def value(row):
                val = row[metric]
                return statistics.median(val) if isinstance(val, list) else val

            groups = {variant: [row for row in rows if row["variant"] == variant] for variant in "AB"}
            deltas = []
            for block in range(args.blocks):
                means = {variant: statistics.mean(value(row) for row in groups[variant] if row["block"] == block)
                         for variant in "AB"}
                deltas.append(means["B"] - means["A"])
            boot = [statistics.mean(rng.choices(deltas, k=len(deltas))) for _ in range(10000)]
            summary[scenario][metric] = {
                "baseline_median": statistics.median(value(row) for row in groups["A"]),
                "candidate_median": statistics.median(value(row) for row in groups["B"]),
                "paired_block_mean_delta": statistics.mean(deltas),
                "paired_block_bootstrap_95_ci": [percentile(boot, .025), percentile(boot, .975)],
                "baseline_p95": percentile([v for row in groups["A"] for v in (row[metric] if isinstance(row[metric], list) else [row[metric]])], .95),
                "candidate_p95": percentile([v for row in groups["B"] for v in (row[metric] if isinstance(row[metric], list) else [row[metric]])], .95),
            }
        summary[scenario]["wrong_folders"] = {
            variant: sum(row["wrong_folders"] for row in rows if row["variant"] == variant) for variant in "AB"
        }
    metadata = {
        "host": platform.node(), "kernel": platform.platform(), "blocks": args.blocks,
        "started_utc": started, "ended_utc": datetime.now(timezone.utc).isoformat(),
        "load_average_before": load_before, "load_average_after": os.getloadavg(),
        "binary_sha256": {variant: hashlib.sha256(path.read_bytes()).hexdigest() for variant, path in binaries.items()},
        "shell_versions": {name: subprocess.check_output([shell, "--version"], text=True).splitlines()[0]
                           for name, shell in [("bash", args.bash), ("fish", args.fish)]},
        "font_directory": os.environ.get("AIZOME_FONT_DIR"),
        "cpu": next(line.split(":", 1)[1].strip() for line in Path("/proc/cpuinfo").read_text().splitlines() if line.startswith("model name")),
        "cpu_governor": Path("/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor").read_text().strip(),
        "power_supplies": {path.parent.name: path.read_text().strip() for path in Path("/sys/class/power_supply").glob("*/status")},
    }
    with lzma.open(args.evidence / "samples.json.xz", "wt") as output:
        json.dump({"metadata": metadata, "samples": samples}, output)
    (args.evidence / "summary.json").write_text(json.dumps({"metadata": metadata, "summary": summary}, indent=2) + "\n")


if __name__ == "__main__":
    main()
