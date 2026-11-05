#!/usr/bin/env python3
"""The controlled comparison's matrix (docs/performance/why-toolkits.md):
every subject under the same private headless sway, trial by trial, the
subjects' order rotated each round.

    run_sway.py OUT ROUNDS SCENARIO[,SCENARIO...] [SUBJECT,...] [--first-round N]

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
    # Typing: the window's text view, the caret at the start of line 20.
    "lat-small": ("latency", f"--keys 150 --args 'text {F}/small.txt'"),
    "lat-large": ("latency", f"--keys 150 --args 'text {F}/large.txt'"),
    "lat-idle-small": ("latency", f"--no-tick --keys 150 --args 'text {F}/small.txt'"),
    "lat240-small": ("latency", f"--hz 240 --keys 150 --args 'text {F}/small.txt'"),
    # The whole output copied every frame, as terminals.md's observer did.
    "lat-fullcopy": ("latency", f"--full-copy --keys 150 --args 'text {F}/small.txt'"),
    "trace-small": ("keys", f"--keys 150 --args 'text {F}/small.txt'"),
    "trace-large": ("keys", f"--keys 150 --args 'text {F}/large.txt'"),
    "cost-small": ("keys", f"--no-tick --keys 150 --rate 10 --args 'text {F}/small.txt'"),
    "cost-large": ("keys", f"--no-tick --keys 150 --rate 10 --args 'text {F}/large.txt'"),
    # Scrolling the 10,000-row list, by the program's own timer.
    "scroll-paced": ("scroll", "--no-tick --args 'list --scroll 1 20'"),
    "scroll-flat": ("scroll", "--no-tick --args 'list --scroll 4 8'"),
    "scroll-page": ("scroll", "--no-tick --args 'list --scroll 0 8'"),
    "resize": ("resize", f"--no-tick --args 'text {F}/small.txt'"),
    "blink": ("idle", f"--no-tick --blink --seconds 8 --args 'text {F}/small.txt'"),
    # 3840 x 2160 at scale 2, the window 1800 x 1000 points.
    "4k-lat": ("latency", f"--res 3840x2160 --scale 2 --keys 150 --args 'text {F}/small.txt --size 1800 1000'"),
    "4k-cost": ("keys", f"--no-tick --res 3840x2160 --scale 2 --keys 150 --rate 10 --args 'text {F}/small.txt --size 1800 1000'"),
    "4k-flat": ("scroll", "--no-tick --res 3840x2160 --scale 2 --args 'list --scroll 4 8 --size 1800 1000'"),
    "4k-page": ("scroll", "--no-tick --res 3840x2160 --scale 2 --args 'list --scroll 0 8 --size 1800 1000'"),
    "4k-ceiling": ("scroll", "--no-tick --res 3840x2160 --scale 2 --hz 1000 --args 'list --scroll 0 1 --size 1800 1000'"),
    "4k-blink": ("idle", f"--no-tick --blink --seconds 8 --res 3840x2160 --scale 2 --args 'text {F}/small.txt --size 1800 1000'"),
}
MATCHED = ["kit", "gtk4", "adw", "qtwidgets", "qtquick"]


def main():
    out, rounds, scen = sys.argv[1], int(sys.argv[2]), sys.argv[3].split(",")
    subs = sys.argv[4].split(",") if len(sys.argv) > 4 and not sys.argv[4].startswith("--") else MATCHED
    first = int(sys.argv[sys.argv.index("--first-round") + 1]) if "--first-round" in sys.argv else 0
    extra = os.environ.get("TK_EXTRA", "")
    for r in range(first, first + rounds):
        for sc in scen:
            kind, args = SCENARIOS[sc]
            order = subs[r % len(subs):] + subs[:r % len(subs)]
            for s in order:
                d = f"{out}/{sc}/{s}-r{r}"
                if os.path.exists(f"{d}/result.json"):
                    continue
                quiet = tk.wait_quiet(1.5, 120)
                t0 = time.time()
                cmd = [sys.executable, f"{here}/sway_trial.py", s, kind, d, "--seed", str(r * 100 + 7),
                       *shlex.split(args), *shlex.split(extra)]
                p = subprocess.run(cmd, capture_output=True, text=True)
                ok = p.returncode == 0
                print(json.dumps(dict(scenario=sc, subject=s, round=r, ok=ok, quiet=quiet, load=tk.loadavg(),
                                      seconds=round(time.time() - t0, 1), err=p.stderr[-400:] if not ok else "")),
                      flush=True)


if __name__ == "__main__":
    main()
