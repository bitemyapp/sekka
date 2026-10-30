#!/usr/bin/env python3
"""Hotaru through Ghostty's own benchmark suite (src/benchmark/AGENTS.md).

ghostty-gen makes each input once; ghostty-bench +terminal-stream and
hotaru-replay --bench then read the same files, timed by hyperfine as whole
commands with warmups, pinned to one CPU, under the workstation lock. Before
timing, each file's final screen state is compared between the pinned
libghostty-vt and hotaru-replay (zero history), so a ratio only stands for
equivalent visible work. An empty file measures each process's fixed cost.

Ghostty's stream benchmark keeps its terminal's default history (10,000
bytes, at least one page); Hotaru is timed with zero and with 1,000 rows of
history to bracket it.
"""
import fcntl
import json
import os
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, "/home/callen/work/sekka-hotaru-settings/apps/hotaru/tools")
from differential import Oracle, differences, digest  # noqa: E402

REF = Path("/home/callen/work/ghostty-reference-20261028")
GEN, BENCH = REF / "zig-out/bin/ghostty-gen", REF / "zig-out/bin/ghostty-bench"
LIBRARY = REF / "zig-out/lib/libghostty-vt.so"
REPLAY = Path("/home/callen/work/sekka-hotaru-settings/apps/hotaru/target/release/hotaru-replay")
WORK = Path("/home/callen/sekka-work/hotaru-bench-20261030/ghostty-suite")
DATA = Path("/dev/shm/hotaru-ghostty-suite")
SIZE = 64 * 1024 * 1024
ROWS, COLS, CPU = 80, 120, "8"
INPUTS = {
    # The generators' own documented examples (src/synthetic/cli/*.zig).
    "ascii": ["+ascii"],
    "styled-plain": ["+styled", "--seed=42"],
    "styled-heavy": ["+styled", "--seed=42", "--style-rate=0.8"],
    "styled-unicode": ["+styled", "--seed=42", "--weight-two=1", "--weight-three=1", "--weight-four=0.5",
                       "--grapheme-rate=0.1"],
    "styled-mixed": ["+styled", "--seed=42", "--style-rate=0.3", "--weight-two=0.5", "--weight-three=0.25",
                     "--weight-four=0.1", "--grapheme-rate=0.05", "--osc8-rate=0.1"],
    "utf8": ["+utf8", "--seed=42"],
    "osc": ["+osc"],
}


def generate(name, args):
    path = DATA / f"{name}.vt"
    with path.open("wb") as out:
        gen = subprocess.Popen([str(GEN), *args], stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
        left = SIZE
        while left:
            chunk = gen.stdout.read(min(left, 1 << 20))
            if not chunk:
                break
            out.write(chunk)
            left -= len(chunk)
        gen.kill()
        gen.wait()
    if path.stat().st_size != SIZE:
        raise RuntimeError(f"{name}: generator stopped at {path.stat().st_size} bytes")
    return path


def main():
    WORK.mkdir(parents=True, exist_ok=False)
    DATA.mkdir(parents=True, exist_ok=True)
    report = {"scope": __doc__, "ghostty_bench_sha256": digest(BENCH), "ghostty_gen_sha256": digest(GEN),
              "library_sha256": digest(LIBRARY), "replay_sha256": digest(REPLAY), "rows": ROWS, "cols": COLS,
              "cpu": CPU, "input_bytes": SIZE, "inputs": {}}
    with Path("/home/callen/.cache/hotaru-workstation.lock").open("a") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        oracle = Oracle(LIBRARY)
        empty = DATA / "empty.vt"
        empty.write_bytes(b"")
        files = {"empty": empty} | {name: generate(name, args) for name, args in INPUTS.items()}
        for name, path in files.items():
            entry = {"generator": INPUTS.get(name, []), "sha256": digest(path), "bytes": path.stat().st_size}
            data = path.read_bytes()
            try:
                reference = oracle.snapshot(data, ROWS, COLS, 65536)
            except RuntimeError as error:
                reference = {"reference_failed": str(error)}
            result = subprocess.run([str(REPLAY), "--data", str(path), "--rows", str(ROWS), "--cols", str(COLS),
                                     "--format", "json"], capture_output=True, text=True, timeout=600)
            actual = json.loads(result.stdout) if result.returncode == 0 else None
            delta = differences(reference, actual) if actual is not None else [result.stderr[-2000:]]
            entry.update(state_matched=not delta, state_differences=delta[:10])
            commands = {
                "ghostty": f"taskset -c {CPU} {BENCH} +terminal-stream --data={path} "
                           f"--terminal-rows={ROWS} --terminal-cols={COLS}",
                "hotaru-history-0": f"taskset -c {CPU} {REPLAY} --bench --data {path} "
                                    f"--rows {ROWS} --cols {COLS} --scrollback 0",
                "hotaru-history-1000": f"taskset -c {CPU} {REPLAY} --bench --data {path} "
                                       f"--rows {ROWS} --cols {COLS} --scrollback 1000",
            }
            export = WORK / f"{name}.hyperfine.json"
            command = ["hyperfine", "-N", "--warmup", "3", "--runs", "20", "--export-json", str(export)]
            for label, text in commands.items():
                command.extend(["--command-name", label, text])
            subprocess.run(command, check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            runs = {r["command"]: r for r in json.loads(export.read_text())["results"]}
            entry["median_ms"] = {label: round(runs[label]["median"] * 1000, 2) for label in commands}
            entry["min_ms"] = {label: round(runs[label]["min"] * 1000, 2) for label in commands}
            entry["commands"] = commands
            entry["load_average"] = os.getloadavg()
            report["inputs"][name] = entry
            (WORK / "report.json").write_text(json.dumps(report, indent=2) + "\n")
            print(name, "state" if entry["state_matched"] else "STATE DIFFERS", entry["median_ms"], flush=True)
    for path in DATA.iterdir():
        path.unlink()
    DATA.rmdir()


if __name__ == "__main__":
    main()
