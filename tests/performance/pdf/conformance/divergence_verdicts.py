#!/usr/bin/env python3
"""Group divergence_gallery.py's page by verdict, with each case's reasoning
under its images (docs/performance/pdf-verdicts.md).

  divergence_verdicts.py OUT_DIR VERDICTS_TSV

OUT_DIR is divergence_gallery.py's output; its index.html is kept as
index-by-category.html and replaced. VERDICTS_TSV is
docs/performance/data/pdf/conformance/pdfium-pixel-verdicts.tsv.
"""
import csv, html, re, sys, os
out, verdicts_tsv = sys.argv[1], sys.argv[2]
src = open(os.path.join(out, "index-by-category.html") if os.path.exists(os.path.join(out, "index-by-category.html")) else os.path.join(out, "index.html")).read()
if not os.path.exists(os.path.join(out, "index-by-category.html")):
    open(os.path.join(out, "index-by-category.html"), "w").write(src)
head = src.split("<h2>", 1)[0]
figs = {}
for m in re.finditer(r'<figure><figcaption><code>([^<]+)</code>(.*?)</figure>', src, re.S):
    figs.setdefault(m.group(1), []).append(m.group(0))
rows = list(csv.DictReader(open(verdicts_tsv), delimiter="\t"))
order = ["PDFium more correct", "hayro fork more correct", "Both wrong", "Both acceptable"]
blurb = {
    "PDFium more correct": "PDFium's drawing is the more correct one; these are what a strict mode would fix.",
    "hayro fork more correct": "The fork's drawing is the more correct one; PDFium's expected image has the bug.",
    "Both wrong": "Neither drawing follows the spec.",
    "Both acceptable": "The spec allows either result, usually because the input is malformed and no result is defined (7 are pages with no MediaBox: hayro uses A4, the others US Letter).",
}
conf_rank = {"high": 0, "medium": 1, "low": 2}
style = "<style>.v{margin:6px 0 10px;padding:8px 10px;background:#f7f7f9;border-left:4px solid #888;font-size:14px}.v b{font-weight:600}.v .c{color:#666}.pdfium{border-color:#c0392b}.hayro{border-color:#2471a3}.both{border-color:#7d3c98}.ok{border-color:#888}table{border-collapse:collapse;background:#fff}td,th{border:1px solid #ccc;padding:4px 10px;text-align:left}</style>"
cls = {"PDFium more correct": "pdfium", "hayro fork more correct": "hayro", "Both wrong": "both", "Both acceptable": "ok"}
parts = [head.replace("</style>", "</style>" + style)]
counts = {v: [r for r in rows if r["verdict"] == v] for v in order}
parts.append("<p>Grouped by which drawing is more correct, judged against ISO 32000-2 with MuPDF 1.27 and Poppler 26.09 as tie-breakers (full reasoning in <code>docs/performance/pdf-verdicts.md</code>). <a href=\"index-by-category.html\">The earlier grouping by symptom</a> misfiled 13 cases.</p>")
t = ["<table><tr><th>Verdict</th><th>All</th><th>High confidence</th><th>Medium</th><th>Low</th></tr>"]
for v in order:
    rs = counts[v]
    t.append(f"<tr><td><a href=\"#{cls[v]}\">{v}</a></td><td>{len(rs)}</td>" + "".join(f"<td>{sum(1 for r in rs if r['confidence'] == c)}</td>" for c in ("high", "medium", "low")) + "</tr>")
t.append("</table>")
parts.append("".join(t))
missing = []
for v in order:
    rs = sorted(counts[v], key=lambda r: (conf_rank.get(r["confidence"], 3), r["test"]))
    parts.append(f"<h2 id=\"{cls[v]}\">{html.escape(v)} ({len(rs)})</h2><p>{html.escape(blurb[v])}</p>")
    for r in rs:
        fs = figs.get(r["test"])
        if not fs:
            missing.append(r["test"]); continue
        note = (f"<div class=\"v {cls[v]}\"><b>{html.escape(v)}</b> <span class=\"c\">({html.escape(r['confidence'])} confidence)</span><br>"
                f"{html.escape(r['reason'])}"
                + (f"<br><span class=\"c\">hayro: {html.escape(r['hayro_cause'])}</span>" if r['hayro_cause'].strip() else "")
                + (f"<br><span class=\"c\">Spec: {html.escape(r['spec_ref'])}</span>" if r['spec_ref'].strip() else "")
                + "</div>")
        for i, f in enumerate(fs):
            parts.append(f.replace("</figcaption>", "</figcaption>" + note, 1) if i == 0 else f)
open(os.path.join(out, "index.html"), "w").write("\n".join(parts))
print("figures", sum(len(v) for v in figs.values()), "tests", len(figs), "verdicts", len(rows), "missing", missing, "unjudged", sorted(set(figs) - {r['test'] for r in rows}))
