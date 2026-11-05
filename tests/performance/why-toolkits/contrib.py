#!/usr/bin/env python3
"""The gaps between the kit and each toolkit, part by part, from analyze.py's
summary (docs/performance/why-toolkits.md's ranked tables).

    contrib.py SUMMARY-SWAY.json > contrib.md

Each part's gap is the toolkit's figure less the kit's (positive: the kit
spends less). Medians across trials."""
import json
import sys

SUBJECTS = ["gtk4", "adw", "qtwidgets", "qtquick"]
NAMES = {"kit": "Kit", "gtk4": "GTK 4", "adw": "libadwaita", "qtwidgets": "Qt Widgets", "qtquick": "Qt Quick"}


def med(s, k):
    v = s.get(k)
    if isinstance(v, dict):
        return v.get("median", v.get("p50"))
    return v


def p(s, k, q="p50"):
    v = s.get(k)
    return v.get(q) if isinstance(v, dict) else None


def row(label, values, unit, d=3):
    cells = []
    for v in values:
        cells.append("–" if v is None else f"{v:+.{d}f}")
    return f"| {label} | " + " | ".join(cells) + f" | {unit} |"


def main():
    S = json.load(open(sys.argv[1]))["summary"]
    out = []
    for sc, per in (("cost-small", "key"), ("cost-large", "key")):
        if sc not in S or "kit" not in S[sc]:
            continue
        k = S[sc]["kit"]
        out.append(f"### {sc}: cost per keystroke, gap over the kit\n")
        out.append("| Part | " + " | ".join(NAMES[s] for s in SUBJECTS) + " | unit |")
        out.append("|---|" + "---:|" * len(SUBJECTS) + "---|")
        for label, key in (("program CPU", "app_cpu_ms_per_key"), ("program GPU", "app_gpu_ms_per_key"),
                           ("compositor CPU", "comp_cpu_ms_per_key"), ("compositor GPU", "comp_gpu_ms_per_key")):
            out.append(row(label, [None if s not in S[sc] else
                                   (med(S[sc][s], key) or 0) - (med(k, key) or 0) for s in SUBJECTS], "ms/key"))
        out.append(f"\nKit: " + ", ".join(f"{key} {med(k, key):.3f}" for key in
                                           ("app_cpu_ms_per_key", "app_gpu_ms_per_key", "comp_cpu_ms_per_key",
                                            "comp_gpu_ms_per_key") if med(k, key) is not None) + "\n")
    for sc in ("scroll-paced", "scroll-flat", "scroll-page", "4k-flat", "4k-page", "4k-ceiling"):
        if sc not in S or "kit" not in S[sc]:
            continue
        k = S[sc]["kit"]
        out.append(f"### {sc}: cost per frame, gap over the kit\n")
        out.append("| Part | " + " | ".join(NAMES[s] for s in SUBJECTS) + " | unit |")
        out.append("|---|" + "---:|" * len(SUBJECTS) + "---|")
        for label, key in (("program CPU", "app_cpu_ms_per_frame"), ("program GPU", "app_gpu_ms_per_frame"),
                           ("compositor CPU", "comp_cpu_ms_per_frame"), ("compositor GPU", "comp_gpu_ms_per_frame")):
            out.append(row(label, [None if s not in S[sc] else
                                   (med(S[sc][s], key) or 0) - (med(k, key) or 0) for s in SUBJECTS], "ms/frame"))
        out.append(row("frames a second", [None if s not in S[sc] else
                                           (med(S[sc][s], "fps") or 0) - (med(k, "fps") or 0) for s in SUBJECTS], "fps", 1))
        out.append("")
    for sc in ("lat-small", "lat-large", "lat240-small", "lat-idle-small", "4k-lat"):
        if sc not in S or "kit" not in S[sc]:
            continue
        k = S[sc]["kit"]
        out.append(f"### {sc}: key to frame, p50 (p99), gap over the kit\n")
        out.append("| | " + " | ".join(NAMES[s] for s in SUBJECTS) + " | unit |")
        out.append("|---|" + "---:|" * len(SUBJECTS) + "---|")
        out.append(row("p50", [None if s not in S[sc] else p(S[sc][s], "latency_ms") - p(k, "latency_ms")
                               for s in SUBJECTS], "ms", 2))
        out.append(row("p99", [None if s not in S[sc] else p(S[sc][s], "latency_ms", "p99") - p(k, "latency_ms", "p99")
                               for s in SUBJECTS], "ms", 2))
        out.append("")
    print("\n".join(out))


if __name__ == "__main__":
    main()
