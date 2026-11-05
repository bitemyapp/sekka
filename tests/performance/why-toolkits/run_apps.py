#!/usr/bin/env python3
"""Real apps under the same private headless sway (docs/performance/why-toolkits.md):
Sumi, GNOME Text Editor and KWrite typed in (the 10,000-line file, at its
start); Hako, Files (Nautilus) and Dolphin scrolled by wheel notches in a
folder of 10,000 files. Alternating, the order rotated each round.

    run_apps.py OUT ROUNDS"""
import json
import os
import shlex
import subprocess
import sys
import time

here = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, here)
import tk  # noqa: E402

F = tk.FIX
EDITORS = ["sumi", "gte", "kwrite"]
FILES = ["hako", "nautilus", "dolphin"]
SCEN = [
    ("app-lat", EDITORS, "latency", f"--keys 150 --args '{F}/large.txt'"),
    ("app-trace", EDITORS, "keys", f"--keys 150 --args '{F}/large.txt'"),
    ("app-cost", EDITORS, "keys", f"--no-tick --keys 150 --rate 10 --args '{F}/large.txt'"),
    ("app-wheel", FILES, "wheel", f"--keys 60 --settle 5 --args '{F}/folder'"),
]


def main():
    out, rounds = sys.argv[1], int(sys.argv[2])
    for r in range(rounds):
        for sc, subs, kind, args in SCEN:
            order = subs[r % len(subs):] + subs[:r % len(subs)]
            for s in order:
                d = f"{out}/{sc}/{s}-r{r}"
                if os.path.exists(f"{d}/result.json"):
                    continue
                quiet = tk.wait_quiet(1.5, 120)
                t0 = time.time()
                cmd = [sys.executable, f"{here}/sway_trial.py", s, kind, d, "--seed", str(r * 100 + 7), *shlex.split(args)]
                p = subprocess.run(cmd, capture_output=True, text=True)
                print(json.dumps(dict(scenario=sc, subject=s, round=r, ok=p.returncode == 0, quiet=quiet,
                                      load=tk.loadavg(), seconds=round(time.time() - t0, 1),
                                      err=p.stderr[-400:] if p.returncode else "")), flush=True)


if __name__ == "__main__":
    main()
