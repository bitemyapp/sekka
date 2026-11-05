#!/usr/bin/env python3
"""The dynamic linker's start-up statistics (LD_DEBUG=statistics) of each
program in probe.py's `lddebug` set: the last block of each run's stderr,
which is the real program's (a Nix wrapper prints its own first).

    lddebug.py RUNDIR

Only the counts are used in why-apps.md: ld.so's own timings (rdtsc
"cycles") came out 6-7 times lower than the profile and the stamps agree
on, so they are listed but not relied on.
"""
import json
import re
import statistics
import sys

KEYS = ("total startup time in dynamic loader", "time needed for relocation", "number of relocations",
        "number of relocations from cache", "number of relative relocations", "time needed to load objects")


def last_block(path):
    blocks, cur = [], {}
    for line in open(path, errors="replace"):
        m = re.search(r"^\s*\d+:\s+(.*?):\s+(\d+)", line)
        if not m:
            continue
        k, v = m.group(1).strip(), int(m.group(2))
        if k == "total startup time in dynamic loader" and cur:
            blocks.append(cur)
            cur = {}
        if k in KEYS:
            cur[k] = v
    if cur:
        blocks.append(cur)
    return blocks[-1] if blocks else {}


def main():
    rundir = sys.argv[1]
    runs = [json.loads(l) for l in open(f"{rundir}/runs.jsonl")]
    runs = [r for r in runs if not r.get("meta") and not r.get("warmup")]
    names = list(dict.fromkeys(r["name"] for r in runs))
    print("| Program | " + " | ".join(KEYS) + " |")
    print("|---|" + "---:|" * len(KEYS))
    for n in names:
        bs = [last_block(f"{rundir}/{n}-{r['rep']}.stderr") for r in runs if r["name"] == n]
        row = [statistics.median(b.get(k, 0) for b in bs) for k in KEYS]
        print(f"| {n} | " + " | ".join(f"{v:,.0f}" for v in row) + " |")


if __name__ == "__main__":
    main()
