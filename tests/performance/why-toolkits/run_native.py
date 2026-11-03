#!/usr/bin/env python3
"""The native comparison's matrix (docs/performance/why-toolkits.md): each
toolkit under its own desktop's compositor, trial by trial, the pairs'
order rotated each round.

    run_native.py OUT ROUNDS SCENARIO[,SCENARIO...] [DESKTOP:SUBJECT,...]

Each trial waits (up to 2 min) for a one-minute load average under 1.5."""
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
SCENARIOS = {
    "keys-small": ("keys", f"--keys 150 --args 'text {F}/small.txt'"),
    "keys-large": ("keys", f"--keys 150 --args 'text {F}/large.txt'"),
    "scroll-paced": ("scroll", "--args 'list --scroll 1 20'"),
    "scroll-flat": ("scroll", "--args 'list --scroll 4 8'"),
    "scroll-page": ("scroll", "--args 'list --scroll 0 8'"),
    # Real apps: the large file, typed at its start.
    "app-keys": ("keys", f"--keys 150 --args '{F}/large.txt'"),
}
PAIRS = ["aizome:kit", "gnome:gtk4", "gnome:adw", "kde:qtwidgets", "kde:qtquick"]


def main():
    out, rounds, scen = sys.argv[1], int(sys.argv[2]), sys.argv[3].split(",")
    pairs = sys.argv[4].split(",") if len(sys.argv) > 4 else PAIRS
    for r in range(rounds):
        for sc in scen:
            kind, args = SCENARIOS[sc]
            order = pairs[r % len(pairs):] + pairs[:r % len(pairs)]
            for pair in order:
                desk, s = pair.split(":")
                d = f"{out}/{sc}/{desk}-{s}-r{r}"
                if os.path.exists(f"{d}/result.json"):
                    continue
                quiet = tk.wait_quiet(1.5, 120)
                t0 = time.time()
                cmd = [sys.executable, f"{here}/native_trial.py", desk, s, kind, d, "--seed", str(r * 100 + 7),
                       *shlex.split(args)]
                p = subprocess.run(cmd, capture_output=True, text=True)
                ok = p.returncode == 0
                print(json.dumps(dict(scenario=sc, pair=pair, round=r, ok=ok, quiet=quiet, load=tk.loadavg(),
                                      seconds=round(time.time() - t0, 1), err=p.stderr[-400:] if not ok else "")),
                      flush=True)


if __name__ == "__main__":
    main()
