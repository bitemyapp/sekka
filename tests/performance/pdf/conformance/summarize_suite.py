#!/usr/bin/env python3
"""Counts a hayro test-suite run (the JSON lines written with
hayro-tests-results-jsonl.patch) per manifest, and grades each failing raster
page by how far it is from its reference image, from the images the suite's
report keeps (report/raster/<kind>/<stem>[_p<N>].png and .ref.png).

  summarize_suite.py RESULTS.jsonl TESTS_DIR REPORT_DIR OUT.json OUT_FAILURES.tsv

The suite itself passes a page when no pixel differs from the reference by
more than 3 in any channel (a manifest's max_diff_pixels lets that many
pixels exceed it). Grading counts, for each failing page, the pixels that
differ by more than 3, 16 and 48 and the largest difference.
"""
import json
import os
import re
import sys
from collections import Counter, defaultdict

import numpy as np
from PIL import Image

KINDS = ["custom", "pdfjs", "pdfbox", "pdfium", "corpus"]


def kind_of(name):
    for k in KINDS[1:]:
        if name.startswith(k + "_"):
            return k, name[len(k) + 1:]
    return "custom", name


def load(path):
    im = Image.open(path)
    if im.mode != "RGBA":
        im = im.convert("RGBA")
    a = np.asarray(im).astype(np.int16)
    return a


def grade(actual, ref):
    a, b = load(actual), load(ref)
    if a.shape != b.shape:
        return {"size_mismatch": True, "actual": list(a.shape[:2]), "reference": list(b.shape[:2])}
    d = np.abs(a - b).max(axis=2)
    n = d.size
    return {"over_3": int((d > 3).sum()), "over_16": int((d > 16).sum()), "over_48": int((d > 48).sum()),
            "frac_over_48": float((d > 48).sum()) / n, "max": int(d.max()), "pixels": n}


def bucket(g):
    if g is None:
        return "no_image"
    if g.get("size_mismatch"):
        return "size_mismatch"
    f = g["frac_over_48"]
    if f == 0:
        return "none_over_48"
    if f < 0.001:
        return "under_0.1%_over_48"
    if f < 0.01:
        return "0.1-1%_over_48"
    if f < 0.05:
        return "1-5%_over_48"
    return "over_5%_over_48"


ORDER = ["none_over_48", "under_0.1%_over_48", "0.1-1%_over_48", "1-5%_over_48", "over_5%_over_48",
         "size_mismatch", "no_image"]


def main():
    jsonl, tests_dir, report_dir, out_json, out_tsv = sys.argv[1:6]
    results = [json.loads(l) for l in open(jsonl)]
    ignored = Counter()
    entries = Counter()
    for k in KINDS:
        for v in json.load(open(os.path.join(tests_dir, f"manifest_{k}.json"))):
            entries[k] += 1
            if isinstance(v, dict) and v.get("ignore"):
                ignored[k] += 1
    summary = {"suites": defaultdict(lambda: defaultdict(Counter))}
    fails = []
    worst_by_kind = defaultdict(Counter)
    for res in results:
        name = res["name"]
        suite, _, rest = name.partition("::")
        if suite in ("raster", "svg"):
            kind, stem = kind_of(rest)
        else:
            suite, kind, stem = "other", name.split("::")[0], name
        summary["suites"][suite][kind][res["outcome"]] += 1
        if res["outcome"] != "failed":
            continue
        msg = res["message"]
        cause = "pixels"
        if "couldn't download reference image" in msg:
            cause = "reference image unavailable (404)"
        elif "missing reference image" in msg:
            cause = "no reference image"
        elif "couldn't load" in msg or "couldn't read" in msg:
            cause = "load error"
        elif "panicked" in msg or "panic" in msg:
            cause = "panic"
        elif "no pages" in msg:
            cause = "no pages"
        pages = [int(p) for p in re.findall(r"page (\d+): ", msg)] or [None]
        worst = None
        graded = []
        if suite == "raster" and cause == "pixels":
            for p in pages:
                stem_file = f"{stem}.png" if p is None else f"{stem}_p{p}.png"
                actual = os.path.join(report_dir, "raster", kind, stem_file)
                ref = actual[:-4] + ".ref.png"
                if not os.path.exists(actual) and p is not None:
                    actual2 = os.path.join(report_dir, "raster", kind, f"{stem}.png")
                    if os.path.exists(actual2):
                        actual, ref = actual2, actual2[:-4] + ".ref.png"
                g = grade(actual, ref) if os.path.exists(actual) and os.path.exists(ref) else None
                graded.append(g)
            order = {b: i for i, b in enumerate(ORDER)}
            worst = max((bucket(g) for g in graded), key=lambda b: order[b])
            worst_by_kind[kind][worst] += 1
        elif suite == "raster":
            worst_by_kind[kind][cause] += 1
        fails.append([suite, kind, name, cause, worst or "", msg.replace("\n", " | ").replace("\t", " ")[:600],
                      json.dumps(graded) if graded else ""])
    out = {"manifest_entries": dict(entries), "manifest_ignored": dict(ignored), "by_suite": {}}
    for suite, kinds in summary["suites"].items():
        out["by_suite"][suite] = {}
        for kind, c in kinds.items():
            row = {"tests_run": sum(c.values()), "passed": c["passed"], "failed": c["failed"],
                   "skipped": c["skipped"], "other": {k: v for k, v in c.items() if k not in ("passed", "failed", "skipped")}}
            if suite == "raster":
                row["ignored_in_manifest"] = ignored[kind]
                row["failed_graded_worst_page"] = {b: worst_by_kind[kind][b] for b in ORDER + [
                    "reference image unavailable (404)", "no reference image", "load error", "panic", "no pages"]
                    if worst_by_kind[kind][b]}
            out["by_suite"][suite][kind] = row
    tot = Counter()
    for kinds in summary["suites"].values():
        for c in kinds.values():
            tot.update(c)
    out["total"] = dict(tot)
    out["ignored_total"] = sum(ignored.values())
    json.dump(out, open(out_json, "w"), indent=1)
    with open(out_tsv, "w") as f:
        f.write("suite\tmanifest\ttest\tcause\tworst_page_grade\tmessage\tgraded_pages\n")
        for row in fails:
            f.write("\t".join(row) + "\n")
    print(json.dumps(out, indent=1))


if __name__ == "__main__":
    main()
