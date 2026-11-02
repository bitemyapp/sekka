#!/usr/bin/env python3
"""PDFium's pixel tests (testing/resources/pixel in pdfium.googlesource.com,
checked out at the branch of the PDFium build used) drawn by PDFium itself (a
control) and by hayro, each page compared with the test's expected PNG.

  pdfium_pixel_tests.py PDFIUM_CHECKOUT WORK_DIR TEST_FONTS_DIR \
      ENGINE=PROGRAM [ENGINE=PROGRAM...]

Each test is a .in template (expanded to a PDF by PDFium's own
testing/tools/fixup_pdf_template.py) or a .pdf, with expected images
<test>_expected[_agg_linux|_agg|_linux].pdf.<page>.png, the most specific one
used as PDFium's pngdiffer.py does for its AGG renderer on Linux. Left out,
as this harness can't reproduce what PDFium's runner does for them: tests in
xfa_specific/ (XFA), tests with an .evt file (input events sent before
drawing) and tests PDFium's SUPPRESSIONS file skips for this configuration
(linux, nov8, agg, freetype). Tests it marks `diff` or `blank` there are run
and reported apart.

A page is compared where the drawing and the expected image have the same
size; expected images with transparency are composited over white, as the
programs write transparent pages. Per page: exact match; PDFium's own fuzzy
rule (largest channel difference <= 3 and mean squared error <= 0.05, or the
test's fuzzy=<delta>,<mse> suppression; its windowed MSE is not checked); and
the share of pixels whose largest channel difference exceeds 48 (the measure
the corpus comparison uses). A test passes a criterion when every page does
and the engine drew as many pages as there are expected images.
"""
import json
import os
import re
import subprocess
import sys
from collections import Counter, defaultdict

import numpy as np
from PIL import Image

SUFFIXES = ["_agg_linux", "_agg", "_linux", ""]
CONFIG = {"platform": {"*", "linux"}, "engine": {"*", "nov8"}, "renderer": {"*", "agg"}, "font": {"*", "freetype"}}


def suppressions(path):
    rules = defaultdict(list)
    section = None
    for line in open(path):
        m = re.match(r"^# (\w+) tests\s*$", line)
        if m:
            section = m.group(1).lower()
            continue
        line = line.split("#", 1)[0].strip()
        if not line or section != "pixel":
            continue
        cols = line.split()
        if len(cols) < 6:
            continue
        name, plat, eng, rend, font, action = cols[:6]
        if (set(plat.split(",")) & CONFIG["platform"] and set(eng.split(",")) & CONFIG["engine"]
                and set(rend.split(",")) & CONFIG["renderer"] and set(font.split(",")) & CONFIG["font"]):
            rules[name].append(action)
    return rules


def read_ppm(path):
    with open(path, "rb") as f:
        data = f.read()
    m = re.match(rb"P6\s+(\d+)\s+(\d+)\s+255\s", data)
    w, h = int(m.group(1)), int(m.group(2))
    return np.frombuffer(data[m.end():m.end() + w * h * 3], dtype=np.uint8).reshape(h, w, 3)


def read_expected(path):
    im = Image.open(path)
    if im.mode in ("RGBA", "LA") or (im.mode == "P" and "transparency" in im.info):
        im = im.convert("RGBA")
        a = np.asarray(im).astype(np.float64)
        alpha = a[..., 3:4] / 255.0
        rgb = a[..., :3] * alpha + 255.0 * (1 - alpha)
        return np.clip(np.rint(rgb), 0, 255).astype(np.uint8)
    return np.asarray(im.convert("RGB"))


def compare(actual, expected, fuzzy):
    if actual.shape != expected.shape:
        return {"size": f"{actual.shape[1]}x{actual.shape[0]} vs {expected.shape[1]}x{expected.shape[0]}"}
    d = np.abs(actual.astype(np.int16) - expected.astype(np.int16))
    dm = d.max(axis=2)
    mse = float((d.astype(np.float64) ** 2).sum() / (3.0 * dm.size))
    delta, max_mse = fuzzy
    return {"exact": bool(dm.max() == 0), "max": int(dm.max()), "mse": round(mse, 4),
            "pdfium_rule": bool(dm.max() == 0 or (dm.max() <= delta and mse <= max_mse)),
            "frac_over_48": float((dm > 48).sum()) / dm.size, "frac_over_16": float((dm > 16).sum()) / dm.size}


def main():
    checkout, work, fonts = sys.argv[1:4]
    engines = dict(a.split("=", 1) for a in sys.argv[4:])
    pixel = os.path.join(checkout, "testing", "resources", "pixel")
    rules = suppressions(os.path.join(checkout, "testing", "SUPPRESSIONS"))
    fixup = os.path.join(checkout, "testing", "tools", "fixup_pdf_template.py")
    gen = os.path.join(work, "gen")
    os.makedirs(gen, exist_ok=True)

    tests, excluded = [], defaultdict(list)
    for d, _, names in os.walk(pixel):
        for n in sorted(names):
            if not re.match(r"^.+[.](in|pdf)$", n):
                continue
            stem = n.rsplit(".", 1)[0]
            rel = os.path.relpath(os.path.join(d, n), pixel)
            if "xfa_specific" in rel:
                excluded["xfa_specific (XFA)"].append(rel)
            elif os.path.exists(os.path.join(d, stem + ".evt")):
                excluded["input events (.evt)"].append(rel)
            elif "skip" in rules.get(n, []):
                excluded["skipped by SUPPRESSIONS"].append(rel)
            else:
                tests.append((d, n, stem, rel))

    results = []
    for d, n, stem, rel in sorted(tests, key=lambda t: t[3]):
        key = rel.replace("/", "__").rsplit(".", 1)[0]
        pdf = os.path.join(gen, key + ".pdf")
        if n.endswith(".in"):
            subprocess.run([sys.executable, "-I", fixup, f"--output-dir={gen}", os.path.join(d, n)],
                           check=True, capture_output=True)
            produced = os.path.join(gen, stem + ".pdf")
            if produced != pdf:
                os.replace(produced, pdf)
        else:
            with open(os.path.join(d, n), "rb") as src, open(pdf, "wb") as dst:
                dst.write(src.read())
        expected = []
        for page in range(10000):
            found = None
            for s in SUFFIXES:
                p = os.path.join(d, f"{stem}_expected{s}.pdf.{page}.png")
                if os.path.exists(p):
                    found = p
                    break
            if not found:
                break
            expected.append(found)
        actions = rules.get(n, [])
        fuzzy = (3, 0.05)
        for a in actions:
            if a.startswith("fuzzy="):
                p = a[len("fuzzy="):].split(",")
                fuzzy = (int(p[0]), float(p[1]) if len(p) > 1 and p[1] else 0.05)
        font_dir = fonts
        if "use_ahem" in d:
            font_dir = os.path.join(checkout, "testing", "resources", "fonts", "ahem")
        elif "use_symbolneu" in d:
            font_dir = os.path.join(checkout, "testing", "resources", "fonts", "symbolneu")
        row = {"test": rel, "expected_pages": len(expected), "suppression": ",".join(actions), "engines": {}}
        for engine, program in engines.items():
            out = os.path.join(work, "out", engine, key)
            os.makedirs(out, exist_ok=True)
            env = dict(os.environ)
            if "use_" in d:
                env["PDFBENCH_NO_RENAME"] = "1"
            try:
                p = subprocess.run(["timeout", "30", program, pdf, out, font_dir], capture_output=True,
                                   env=env, timeout=40)
                status = p.returncode
                lines = p.stdout.decode("utf-8", "replace").splitlines()
            except subprocess.TimeoutExpired:
                status, lines = 124, []
            drawn = sorted(int(l.split("\t")[1]) for l in lines if l.startswith("page\t"))
            err = next((l.split("\t", 1)[1] for l in lines if l.startswith("error\t")), None)
            pages = []
            for i, exp in enumerate(expected):
                ppm = os.path.join(out, f"{i}.ppm")
                if i not in drawn or not os.path.exists(ppm):
                    pages.append({"page": i, "missing": True})
                    continue
                c = compare(read_ppm(ppm), read_expected(exp), fuzzy)
                c["page"] = i
                pages.append(c)
            ok_pages = len(drawn) == len(expected) and len(expected) > 0
            res = {"status": status, "error": err, "pages_drawn": len(drawn), "pages": pages}
            for crit in ("exact", "pdfium_rule"):
                res[crit] = ok_pages and all(pg.get(crit) for pg in pages)
            for name, lim in (("under_0.1%_over_48", 0.001), ("under_1%_over_48", 0.01), ("under_5%_over_48", 0.05)):
                res[name] = ok_pages and all("frac_over_48" in pg and pg["frac_over_48"] < lim for pg in pages)
            fr = [pg["frac_over_48"] for pg in pages if "frac_over_48" in pg]
            res["worst_frac_over_48"] = max(fr) if fr else None
            row["engines"][engine] = res
        results.append(row)

    with open(os.path.join(work, "pixel-results.json"), "w") as f:
        json.dump({"excluded": excluded, "results": results}, f, indent=1)

    summary = {"tests_found": len(tests) + sum(len(v) for v in excluded.values()),
               "excluded": {k: len(v) for k, v in excluded.items()}, "run": len(results)}
    groups = {"all run": results,
              "not suppressed": [r for r in results if not r["suppression"] or r["suppression"].startswith("fuzzy")],
              "suppressed diff/blank": [r for r in results if r["suppression"] and not r["suppression"].startswith("fuzzy")],
              "no expected images": [r for r in results if r["expected_pages"] == 0]}
    for g, rs in groups.items():
        summary[g] = {"tests": len(rs)}
        for engine in engines:
            c = Counter()
            for r in rs:
                e = r["engines"][engine]
                for crit in ("exact", "pdfium_rule", "under_0.1%_over_48", "under_1%_over_48", "under_5%_over_48"):
                    c[crit] += bool(e[crit])
                c["drew_no_page"] += e["pages_drawn"] == 0
                c["page_count_differs"] += e["pages_drawn"] != r["expected_pages"]
                c["size_differs"] += any("size" in pg for pg in e["pages"])
            summary[g][engine] = dict(c)
    with open(os.path.join(work, "pixel-summary.json"), "w") as f:
        json.dump(summary, f, indent=1)
    print(json.dumps(summary, indent=1))


if __name__ == "__main__":
    main()
