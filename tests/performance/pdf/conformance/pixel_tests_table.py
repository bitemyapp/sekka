#!/usr/bin/env python3
"""Writes pdfium-pixel-tests.tsv from pdfium_pixel_tests.py's pixel-results.json:
one line per test, with how hayro's failures at the 1% criterion were
grouped after looking at each test's expected image beside hayro's (a
judgement made by eye at thumbnail size, recorded here).

  pixel_tests_table.py PIXEL_RESULTS.json OUT.tsv
"""
import json
import sys

CATEGORIES = {
    "page size: a missing or invalid page box (hayro falls back to A4, PDFium to US Letter) or another box chosen":
        "bug_1484283 bug_335309995 bug_527174 bug_551460 bug_554151 bug_71 bug_714187 bug_981347 direct_content_stream",
    "not opened: a damaged file PDFium repairs and hayro refuses": "bug_828206 bug_830221",
    "damaged content: a stream whose /Length is wrong, which PDFium reads to its endstream":
        "bug_543018_1 bug_543018_2 bug_551258_1 bug_524043_3 bug_524043_4 bug_524043_5",
    "no appearance stream: an annotation or form field PDFium draws an appearance for and hayro leaves out":
        "freetext_annotation_with_da freetext_annotation_without_da checkboxes text_form_custom_font bug_733528",
    "edges: glyph or stroke edges at 72 dpi, the drawing otherwise the same":
        "bug_846 bug_1922 bug_736703 dashed_lines bug_1966",
    "drawn differently: content missing, extra, or in other colours or places":
        "bug_557223 bug_1478366 bug_372749732 bug_603518 bug_1693 bug_512557 jpxdecode_with_mismatch_colorspace "
        "bug_867501 shade bug_1546 bug_1746 bug_1395648 radial_shading_point_at_border bug_467170761 bug_332462378 "
        "bug_1321 image_transformer_other bug_1983 shade-tensor same_color_knockout_fill bug_1099446 bug_432208037 "
        "bug_42270459",
}
CAT = {t: c for c, ts in CATEGORIES.items() for t in ts.split()}


def main():
    d = json.load(open(sys.argv[1]))
    with open(sys.argv[2], "w") as f:
        f.write("test\tsuppression\texpected_pages\tfork_pages\tfork_exact\tfork_within_3_mse_0.05\t"
                "fork_worst_frac_over_48\tupstream_exact\tupstream_worst_frac_over_48\tpdfium_exact\t"
                "pdfium_worst_frac_over_48\tfork_failure_at_1%_by_eye\n")
        for r in d["results"]:
            h, p = r["engines"]["hayro-fork"], r["engines"]["pdfium"]
            u = r["engines"].get("hayro-upstream")
            stem = r["test"].rsplit("/", 1)[-1].rsplit(".", 1)[0]
            frac = lambda e: "" if e is None or e["worst_frac_over_48"] is None else f"{e['worst_frac_over_48']:.5f}"
            yes = lambda e, k: "" if e is None else ("yes" if e[k] else "no")
            row = [r["test"], r["suppression"] or "-", r["expected_pages"], h["pages_drawn"],
                   yes(h, "exact"), yes(h, "pdfium_rule"), frac(h), yes(u, "exact"), frac(u),
                   yes(p, "exact"), frac(p),
                   "" if h["under_1%_over_48"] or r["suppression"] else CAT.get(stem, "unclassified")]
            f.write("\t".join(map(str, row)) + "\n")


if __name__ == "__main__":
    main()
