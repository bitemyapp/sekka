#!/usr/bin/env python3
"""Summaries of the real-world documents' runs and of Shiori's own path.

  analyze_extra.py RESULTS_DIR OUT_DIR
"""
import csv
import json
import os
import re
import statistics
import sys
from collections import defaultdict

import numpy as np


def r(x, n=2):
    return None if x is None else round(x, n)


def main():
    res, out = sys.argv[1], sys.argv[2]

    # Real-world documents: first 20 pages, five runs; a page's time is its median.
    pages = os.path.join(res, "realworld-pages.tsv")
    if os.path.exists(pages):
        t = defaultdict(list)
        with open(pages) as f:
            for tag, engine, name, page, ms, w, h in csv.reader(f, delimiter="\t"):
                t[(engine, name, int(page))].append(float(ms))
        med = {k: statistics.median(v) for k, v in t.items()}
        docs = sorted({k[1] for k in med})
        engines = ["pdfium", "hayro-fork", "hayro-upstream", "mupdf", "poppler-splash", "poppler-cairo"]
        states = defaultdict(set)
        with open(os.path.join(res, "realworld-files.tsv")) as f:
            for row in csv.reader(f, delimiter="\t"):
                if row[3] != "ok":
                    states[(row[1], row[2])].add(f"{row[0]}:{row[3]}")
        summary = {"documents": {}, "totals": {}}
        for d in docs:
            summary["documents"][d] = {}
            for e in engines:
                vals = [m for (en, n, p), m in med.items() if en == e and n == d]
                summary["documents"][d][e] = {
                    "pages": len(vals), "total_ms": r(sum(vals), 1),
                    "p50": r(float(np.percentile(vals, 50))) if vals else None, "max": r(max(vals)) if vals else None,
                    "not_ok": sorted(states.get((e, d), [])),
                }
        common = None
        for e in engines:
            keys = {(n, p) for (en, n, p) in med if en == e}
            common = keys if common is None else common & keys
        ref = sum(med[("pdfium", n, p)] for n, p in common)
        for e in engines:
            tot = sum(med[(e, n, p)] for n, p in common)
            summary["totals"][e] = {"pages_all_drew": len(common), "total_ms": r(tot, 0), "ratio_to_pdfium": r(tot / ref, 3),
                                    "faster_than_pdfium_on": sum(1 for n, p in common if med[(e, n, p)] < med[("pdfium", n, p)])}
        with open(os.path.join(out, "realworld-summary.json"), "w") as f:
            json.dump(summary, f, indent=1)

    # Shiori's examples/corpus.rs: one line a file per run.
    runs = sorted(p for p in os.listdir(res) if re.match(r"shiori-corpus-run\d+\.tsv$", p))
    if runs:
        per = defaultdict(list)
        what = defaultdict(list)
        for run in runs:
            with open(os.path.join(res, run)) as f:
                for line in f:
                    parts = line.rstrip("\n").split("\t")
                    if len(parts) < 3:
                        continue
                    name, ms, outcome = parts[0], float(parts[1].split()[0]), parts[2]
                    per[name].append(ms)
                    what[name].append(outcome)
        completed = [n for n in per if all(re.fullmatch(r"[1-9]\d* pages", o) for o in what[n])]
        times = [statistics.median(per[n]) for n in completed]
        other = defaultdict(list)
        for n in per:
            if n in completed:
                continue
            o = what[n][0]
            key = ("open-failed" if o.startswith("open-failed") else "0 pages" if o == "0 pages" else
                   "unanswered" if o.startswith("unanswered") else "draw-failed" if "draw-failed" in o else
                   "worker failed" if " on " in o else "other")
            other[key].append(f"{n}: {o}"[:200])
        summary = {
            "runs": len(runs), "files": len(per), "completed_in_every_run": len(completed),
            "per_file_ms_completed": {"p50": r(float(np.percentile(times, 50)), 1), "p90": r(float(np.percentile(times, 90)), 1),
                                      "p99": r(float(np.percentile(times, 99)), 1), "max": r(max(times), 1)},
            "total_s_completed": r(sum(times) / 1000, 2),
            "not_completed": {k: sorted(v) for k, v in other.items()},
            "outcomes_differ_between_runs": sorted(n for n in per if len(set(what[n])) > 1),
        }
        with open(os.path.join(out, "shiori-corpus-summary.json"), "w") as f:
            json.dump(summary, f, indent=1)
        with open(os.path.join(out, "shiori-corpus.tsv"), "w") as f:
            f.write("file\tmedian_ms\truns_ms\toutcome\n")
            for n in sorted(per):
                f.write(f"{n}\t{statistics.median(per[n]):.0f}\t{','.join(f'{x:.0f}' for x in per[n])}\t{what[n][0]}\n")


if __name__ == "__main__":
    main()
