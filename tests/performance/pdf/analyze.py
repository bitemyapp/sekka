#!/usr/bin/env python3
"""Summarizes the measured rounds (results/pages.tsv, files.tsv, power/*.json,
rounds.log) and the pixel comparison (agreement.tsv) into summary.json and
page-medians.csv in OUT_DIR.

  analyze.py RESULTS_DIR OUT_DIR
"""
import csv
import glob
import json
import os
import statistics
import sys
from collections import defaultdict

import numpy as np

ENGINES = ["pdfium", "hayro-fork", "hayro-upstream", "mupdf", "poppler-splash", "poppler-cairo"]
REF = "pdfium"


def pct(values, q):
    return float(np.percentile(np.asarray(values, dtype=float), q)) if values else None


def r(x, n=2):
    return None if x is None else round(x, n)


def main():
    res, out = sys.argv[1], sys.argv[2]
    os.makedirs(out, exist_ok=True)

    # Per-page times: median of the runs that drew the page.
    times = defaultdict(list)
    size = {}
    tags = set()
    with open(os.path.join(res, "pages.tsv")) as f:
        for tag, engine, name, page, ms, w, h in csv.reader(f, delimiter="\t"):
            tags.add(tag)
            key = (name, int(page))
            times[(engine, key)].append(float(ms))
            size[(engine, key)] = (int(w), int(h))
    tags = sorted(tags)
    med = {k: statistics.median(v) for k, v in times.items()}
    by_engine = defaultdict(dict)
    for (engine, key), m in med.items():
        by_engine[engine][key] = m

    with open(os.path.join(out, "page-medians.csv"), "w", newline="") as f:
        w = csv.writer(f)
        w.writerow(["engine", "file", "page", "median_ms", "runs_drawn", "width", "height"])
        for (engine, key), m in sorted(med.items()):
            w.writerow([engine, key[0], key[1], f"{m:.3f}", len(times[(engine, key)]), *size[(engine, key)]])

    # Per-file process records.
    files = defaultdict(list)  # (engine, name) -> rows
    with open(os.path.join(res, "files.tsv")) as f:
        for row in csv.reader(f, delimiter="\t"):
            tag, engine, name, state, code, wall, rss, open_ms, npages, drawn, failed, detail = row
            files[(engine, name)].append(dict(tag=tag, state=state, code=int(code), wall=float(wall),
                                              rss=int(rss), drawn=int(drawn), failed=int(failed), detail=detail))

    engines = [e for e in ENGINES if e in by_engine]
    common = set.intersection(*(set(by_engine[e]) for e in engines))
    summary = {"runs": tags, "engines": {}, "common_set": {}}

    for e in engines:
        pages = by_engine[e]
        vals = list(pages.values())
        states = defaultdict(lambda: defaultdict(list))
        for (engine, name), rows in files.items():
            if engine != e:
                continue
            for row in rows:
                if row["state"] != "ok":
                    states[row["state"]][name].append(f'{row["tag"]}:{row["code"]}')
        page_fail_files = sorted({name for (engine, name), rows in files.items()
                                  if engine == e and any(r_["failed"] for r_ in rows)})
        rss = [statistics.median(r_["rss"] for r_ in rows) for (engine, name), rows in files.items() if engine == e]
        GIANT = ("issue19517.pdf", "bomb_giant.pdf")
        rss_giant_out = [statistics.median(r_["rss"] for r_ in rows) for (engine, name), rows in files.items()
                         if engine == e and name not in GIANT]
        rss_opened = [statistics.median(r_["rss"] for r_ in rows) for (engine, name), rows in files.items()
                      if engine == e and rows[0]["state"] != "refused"]
        info = {
            "pages_drawn": len(pages),
            "ms_per_page": {"p50": r(pct(vals, 50)), "p90": r(pct(vals, 90)), "p99": r(pct(vals, 99)),
                            "max": r(max(vals)), "mean": r(statistics.mean(vals))},
            "total_ms_all_pages_drawn": r(sum(vals), 0),
            "files_not_ok": {state: {n: v for n, v in sorted(d.items())} for state, d in states.items()},
            "files_with_pages_that_failed": page_fail_files,
            "peak_rss_mib_per_process": {"max": r(max(rss) / 1024, 1), "median": r(statistics.median(rss) / 1024, 1),
                                         "p90": r(pct(rss, 90) / 1024, 1),
                                         "median_of_files_opened": r(statistics.median(rss_opened) / 1024, 1),
                                         "max_without_issue19517_and_bomb_giant": r(max(rss_giant_out) / 1024, 1)},
        }
        if e != REF and REF in by_engine:
            both = set(pages) & set(by_engine[REF])
            ref_t = sum(by_engine[REF][k] for k in both)
            e_t = sum(pages[k] for k in both)
            faster = sum(1 for k in both if pages[k] < by_engine[REF][k])
            behind = [(pages[k] - by_engine[REF][k]) for k in both if pages[k] > by_engine[REF][k]]
            under5 = [k for k in both if pages[k] < 5000 and by_engine[REF][k] < 5000]
            ratios = [pages[k] / by_engine[REF][k] for k in both if by_engine[REF][k] > 0]
            info["against_pdfium_pages_both_drew"] = {
                "pages": len(both), "total_ms": r(e_t, 0), "pdfium_total_ms": r(ref_t, 0),
                "ratio": r(e_t / ref_t, 3),
                "under_5_s_in_both": {"pages": len(under5),
                                      "ratio": r(sum(pages[k] for k in under5) / sum(by_engine[REF][k] for k in under5), 3)},
                "faster_than_pdfium_on": faster,
                "behind_pdfium_on": len(behind), "behind_by_ms_in_all": r(sum(behind), 0),
                "per_page_time_over_pdfium": {"p10": r(pct(ratios, 10)), "p50": r(pct(ratios, 50)),
                                               "p90": r(pct(ratios, 90)), "p99": r(pct(ratios, 99))},
            }
        summary["engines"][e] = info

    ref_common = sum(by_engine[REF][k] for k in common)
    summary["common_set"] = {"pages_all_engines_drew": len(common), "engines": {}}
    for e in engines:
        tot = sum(by_engine[e][k] for k in common)
        cvals = [by_engine[e][k] for k in common]
        summary["common_set"]["engines"][e] = {
            "total_ms": r(tot, 0), "ratio_to_pdfium": r(tot / ref_common, 3),
            "faster_than_pdfium_on": sum(1 for k in common if by_engine[e][k] < by_engine[REF][k]) if e != REF else None,
            "p50": r(pct(cvals, 50)), "p90": r(pct(cvals, 90)), "p99": r(pct(cvals, 99)), "max": r(max(cvals)),
        }

    # Run to run: each run's total over the pages every engine drew.
    per_run = defaultdict(lambda: defaultdict(float))
    with open(os.path.join(res, "pages.tsv")) as f:
        for tag, engine, name, page, ms, w, h in csv.reader(f, delimiter="\t"):
            if (name, int(page)) in common:
                per_run[engine][tag] += float(ms)
    for e in engines:
        summary["common_set"]["engines"][e]["total_ms_by_run"] = {t: r(v, 0) for t, v in sorted(per_run[e].items())}

    # The pages the earlier records named as the fork's remaining gaps to PDFium.
    named = [("issue2948.pdf", 0, "free-form mesh"), ("personwithdog.pdf", 0, "CMYK patch mesh"),
             ("issue6231_1.pdf", 0, "mesh coloured by a stitching function"),
             ("bug1755507.pdf", 0, "a JPEG under a soft mask"), ("issue18032.pdf", 0, "soft mask"),
             ("issue12798_page1_reduced.pdf", 0, "nested transparency groups"),
             ("issue5044.pdf", 0, "soft masks and groups"), ("issue18894.pdf", 0, "soft mask"),
             ("issue10339_reduced.pdf", 0, "Lab colour space set-up"), ("issue2761.pdf", 0, "Lab colour space set-up"),
             ("issue16316.pdf", 0, "Type 1 font parsing"), ("issue8092.pdf", 0, "shadings through gradient tables"),
             ("large_jpeg_downscale.pdf", 0, "JPEG decoding"), ("radial_gradients.pdf", 2, "radial gradients"),
             ("radial_gradients.pdf", 4, "radial gradients"), ("issue918.pdf", 0, "Type 3 image glyphs"),
             ("function_based_shading.pdf", 0, "PostScript function shading"),
             ("vertical_rotated_fallback_glyphs.pdf", 0, "predefined CJK CMaps"),
             ("tracemonkey.pdf", 2, "text (pdf.js's tracemonkey paper)")]
    summary["named_pages"] = [
        {"file": n, "page": p, "what": what,
         **{e: r(by_engine[e].get((n, p))) for e in engines}} for n, p, what in named]

    # Pages where the fork is furthest behind PDFium (time lost).
    if "hayro-fork" in by_engine:
        both = set(by_engine["hayro-fork"]) & set(by_engine[REF])
        lost = sorted(both, key=lambda k: by_engine[REF][k] - by_engine["hayro-fork"][k])[:25]
        summary["hayro_fork_furthest_behind_pdfium"] = [
            {"file": k[0], "page": k[1], "hayro_fork_ms": r(by_engine["hayro-fork"][k]), "pdfium_ms": r(by_engine[REF][k]),
             "hayro_upstream_ms": r(by_engine.get("hayro-upstream", {}).get(k))} for k in lost]
        ahead = sorted(both, key=lambda k: by_engine["hayro-fork"][k] - by_engine[REF][k])[:10]
        summary["hayro_fork_furthest_ahead_of_pdfium"] = [
            {"file": k[0], "page": k[1], "hayro_fork_ms": r(by_engine["hayro-fork"][k]), "pdfium_ms": r(by_engine[REF][k])} for k in ahead]

    # Energy: each engine's whole pass under the PPT meter, against the round's
    # idle baseline. Rounds r* ran over the whole corpus, e* over the files every
    # engine finished alike, five times over (with the null program), p1 a pilot
    # over them once, j* pdf.js with PDFium.
    power = {}
    for path in sorted(glob.glob(os.path.join(res, "power", "*.json"))):
        name = os.path.basename(path)[:-5]
        rnd, subject = name.split("-", 1)
        with open(path) as f:
            power.setdefault(rnd, {})[subject] = json.load(f)
    groups = {"r": "whole_corpus", "e": "common_files", "j": "pdfjs_rounds", "p": "common_files_once_pilot"}
    energy = defaultdict(lambda: defaultdict(list))
    idle_w = {}
    for rnd, subjects in sorted(power.items()):
        idles = [subjects[s]["mean_w"] for s in ("idle-a", "idle-b") if s in subjects]
        idle_w[rnd] = statistics.mean(idles) if idles else None
        for e, p in subjects.items():
            if e.startswith("idle"):
                continue
            dur = p["duration_s"]
            delta = p["energy_j"] - idle_w[rnd] * dur if idle_w[rnd] is not None else None
            energy[groups[rnd[0]]][e].append({"round": rnd, "energy_j": p["energy_j"], "duration_s": dur,
                                               "mean_w": p["mean_w"], "idle_w": r(idle_w[rnd]),
                                               "over_idle_j": r(delta, 1), "busy_cpu_s": p.get("system_busy_cpu_s")})
    summary["energy"] = {}
    for group, by_e in energy.items():
        summary["energy"][group] = {}
        null = by_e.get("null")
        null_j = statistics.median(x["energy_j"] for x in null) if null else None
        for e, rows in by_e.items():
            med = statistics.median(x["energy_j"] for x in rows)
            summary["energy"][group][e] = {
                "rounds": rows,
                "median_energy_j": r(med, 1),
                "median_over_idle_j": r(statistics.median(x["over_idle_j"] for x in rows), 1),
                "median_over_null_j": r(med - null_j, 1) if null_j is not None and e != "null" else None,
                "median_duration_s": r(statistics.median(x["duration_s"] for x in rows), 2),
                "median_mean_w": r(statistics.median(x["mean_w"] for x in rows), 1),
                "median_busy_cpu_s": r(statistics.median(x["busy_cpu_s"] for x in rows), 1),
            }
    summary["idle_w_by_round"] = {k: r(v) for k, v in idle_w.items()}

    # pdf.js: its own rounds (j*), with PDFium in the same rounds.
    pj = os.path.join(res, "pages-pdfjs.tsv")
    if os.path.exists(pj):
        jt = defaultdict(list)
        with open(pj) as f:
            for tag, engine, name, page, ms, w, h in csv.reader(f, delimiter="\t"):
                jt[(engine, (name, int(page)))].append(float(ms))
        jm = defaultdict(dict)
        for (engine, key), v in jt.items():
            jm[engine][key] = statistics.median(v)
        if "pdfjs" in jm and "pdfium" in jm:
            vals = list(jm["pdfjs"].values())
            both = set(jm["pdfjs"]) & set(jm["pdfium"])
            jtot, ptot = sum(jm["pdfjs"][k] for k in both), sum(jm["pdfium"][k] for k in both)
            states = defaultdict(set)
            with open(os.path.join(res, "files-pdfjs.tsv")) as f:
                for row in csv.reader(f, delimiter="\t"):
                    if row[1] == "pdfjs" and row[3] != "ok":
                        states[row[3]].add(row[2])
            summary["pdfjs"] = {
                "pages_drawn": len(vals),
                "ms_per_page": {"p50": r(pct(vals, 50)), "p90": r(pct(vals, 90)), "p99": r(pct(vals, 99)), "max": r(max(vals))},
                "pdfium_in_the_same_rounds_ms_per_page": {q: r(pct(list(jm["pdfium"].values()), n)) for q, n in (("p50", 50), ("p90", 90), ("p99", 99))},
                "against_pdfium_pages_both_drew": {"pages": len(both), "total_ms": r(jtot, 0), "pdfium_total_ms": r(ptot, 0),
                                                   "ratio": r(jtot / ptot, 3),
                                                   "faster_than_pdfium_on": sum(1 for k in both if jm["pdfjs"][k] < jm["pdfium"][k])},
                "on_the_common_set": {"pages": len(common & set(jm["pdfjs"])),
                                      "total_ms": r(sum(jm["pdfjs"][k] for k in common if k in jm["pdfjs"]), 0)},
                "files_not_ok": {k: sorted(v) for k, v in states.items()},
                "pdfium_total_r_rounds_vs_j_rounds_on_shared_pages": r(
                    sum(by_engine[REF][k] for k in both if k in by_engine[REF]) /
                    sum(jm["pdfium"][k] for k in both if k in by_engine[REF]), 3),
            }
            with open(os.path.join(out, "page-medians-pdfjs.csv"), "w", newline="") as f:
                w = csv.writer(f)
                w.writerow(["engine", "file", "page", "median_ms", "runs_drawn"])
                for engine in ("pdfjs", "pdfium"):
                    for key, m in sorted(jm[engine].items()):
                        w.writerow([engine, key[0], key[1], f"{m:.3f}", len(jt[(engine, key)])])

    # Pixel agreement with PDFium.
    agr = os.path.join(res, "agreement.tsv")
    if os.path.exists(agr):
        summary["agreement_with_pdfium"] = {}
        rows = defaultdict(list)
        for path in (agr, os.path.join(res, "agreement-pdfjs.tsv")):
            if not os.path.exists(path):
                continue
            with open(path) as f:
                for row in csv.DictReader(f, delimiter="\t"):
                    rows[row["engine"]].append(row)
        for e, rs in rows.items():
            comp = [x for x in rs if x["compared"] == "1"]
            large = [x for x in rs if x["mean_abs_diff"] == "too large"]
            fr = [float(x["differing_frac"]) for x in comp]
            summary["agreement_with_pdfium"][e] = {
                "pages_both_drew": len(rs), "size_mismatch": len(rs) - len(comp) - len(large),
                "too_large_to_compare": len(large), "compared": len(comp),
                "pixels_differing_by_over_48": {
                    "none": sum(1 for v in fr if v == 0),
                    "under_0.1%": sum(1 for v in fr if v < 0.001),
                    "0.1-1%": sum(1 for v in fr if 0.001 <= v < 0.01),
                    "1-5%": sum(1 for v in fr if 0.01 <= v < 0.05),
                    "over_5%": sum(1 for v in fr if v >= 0.05),
                },
                "median_fraction": r(statistics.median(fr) * 100, 4) if fr else None,
            }

    fu = os.path.join(res, "agreement-fork-vs-upstream.tsv")
    if os.path.exists(fu):
        with open(fu) as f:
            rs = [x for x in csv.DictReader(f, delimiter="\t")]
        comp = [x for x in rs if x["compared"] == "1"]
        fr = [float(x["differing_frac"]) for x in comp]
        summary["agreement_fork_with_upstream"] = {
            "pages_both_drew": len(rs), "compared": len(comp),
            "identical": sum(1 for x in comp if x["max_diff"] == "0"),
            "pixels_differing_by_over_48": {
                "under_0.1%": sum(1 for v in fr if v < 0.001), "0.1-1%": sum(1 for v in fr if 0.001 <= v < 0.01),
                "1-5%": sum(1 for v in fr if 0.01 <= v < 0.05), "over_5%": sum(1 for v in fr if v >= 0.05)},
        }

    # Load during the rounds.
    log = os.path.join(res, "rounds.log")
    if os.path.exists(log):
        summary["load_averages"] = [l.strip() for l in open(log)]

    with open(os.path.join(out, "summary.json"), "w") as f:
        json.dump(summary, f, indent=1)
    print(json.dumps({e: {k: summary["engines"][e][k] for k in ("pages_drawn", "ms_per_page")} for e in engines}, indent=1))
    print(json.dumps(summary["common_set"], indent=1))


if __name__ == "__main__":
    main()
