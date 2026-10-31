#!/usr/bin/env python3
"""Recheck the final screens of the Ghostty suite's generated inputs.

Regenerates each 64 MiB input of ../ghostty-suite/run.py with the same
ghostty-gen arguments and compares the final screen between the pinned
libghostty-vt and hotaru-replay (zero history, 120x80), without timing.
The random UTF-8 and styled inputs are seeded and reproduce the recorded
files; the ASCII and OSC generators take time-based seeds. Paths are wx's.

Usage: suite-recheck.py REPLAY REPORT
"""
import json
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, "/home/callen/work/sekka-hotaru-settings/apps/hotaru/tools")
from differential import Oracle, differences, digest  # noqa: E402

REF = Path("/home/callen/work/ghostty-reference-20261028")
GEN, LIBRARY = REF / "zig-out/bin/ghostty-gen", REF / "zig-out/lib/libghostty-vt.so"
DATA = Path("/dev/shm/hotaru-suite-recheck")
ROWS, COLS, SIZE = 80, 120, 64 << 20
INPUTS = {
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


def main():
    replay, report_path = Path(sys.argv[1]), Path(sys.argv[2])
    DATA.mkdir(exist_ok=True)
    oracle = Oracle(LIBRARY)
    report = {"scope": __doc__, "ghostty_gen_sha256": digest(GEN), "library_sha256": digest(LIBRARY),
              "replay_sha256": digest(replay), "rows": ROWS, "cols": COLS, "input_bytes": SIZE, "inputs": {}}
    for name, args in INPUTS.items():
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
        data = path.read_bytes()
        result = subprocess.run([str(replay), "--data", str(path), "--rows", str(ROWS), "--cols", str(COLS),
                                 "--format", "json"], capture_output=True, text=True, timeout=600, check=True)
        delta = differences(oracle.snapshot(data, ROWS, COLS, 65536), json.loads(result.stdout))
        report["inputs"][name] = {"generator": args, "sha256": digest(path), "bytes": len(data),
                                  "state_matched": not delta, "state_differences": delta[:10]}
        print(name, "matched" if not delta else "DIFFERS", flush=True)
        path.unlink()
    DATA.rmdir()
    report_path.write_text(json.dumps(report, indent=2) + "\n")


if __name__ == "__main__":
    main()
