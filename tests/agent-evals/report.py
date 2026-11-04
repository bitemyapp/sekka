"""Summarise agent-evaluation results as Markdown tables.

    python3 report.py .work/evals/runs/RUN [RUN…] > report.md

Episodes whose fault didn't take (`invalid`) or whose harness failed are
counted apart and excluded from the rates. When a model has several
results for one scenario (repeats, reruns), the latest per repeat counts.
"""

import json
import sys
from collections import defaultdict
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent / "harness"))
from vm import refusal  # noqa: E402


def load(paths):
    """Every episode's result.json under the given run directories. The
    label is the directory the episode is in (the model, +skill)."""
    latest = {}
    for p in paths:
        for f in sorted(Path(p).glob("**/result.json")):
            r = json.loads(f.read_text())
            r.setdefault("label", f.parent.parent.name)
            r.setdefault("prompt_version", "v1")
            # Refusals re-checked with the current patterns (an early one
            # also matched words inside grep patterns).
            r["blocked_commands"] = [c for c in r.get("blocked_commands") or [] if refusal(c)]
            key = (str(f.parent.parent), r["scenario"], r.get("repeat", 0), r.get("observe")
                   if r.get("track") == "computer-use" else None)
            latest[key] = r
    return list(latest.values())


def pct(n, d):
    return f"{100 * n / d:.0f}%" if d else "–"


def mean(xs):
    xs = [x for x in xs if x is not None]
    return sum(xs) / len(xs) if xs else None


def fmt(x, digits=0):
    return "–" if x is None else f"{x:.{digits}f}"


def label_of(r):
    label = r.get("label") or r.get("model") or r.get("agent")
    if r.get("track") == "computer-use" and r.get("observe"):
        label += f" [{r['observe']}]"
    return label


def summary(results, track):
    rows = defaultdict(list)
    for r in results:
        if r.get("track", "diagnostics" if not r["scenario"].startswith("cu-") else "computer-use") == track:
            rows[label_of(r)].append(r)
    out = []
    if track == "diagnostics":
        out.append("| Model | Episodes | Success | Fixed | Root cause named | Collateral | Unfinished | "
                   "Median minutes | Steps | Tokens out / episode | Text tool calls |")
        out.append("|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|")
    else:
        out.append("| Model [observation] | Episodes | Success | Collateral | Unfinished | Median minutes | Actions | "
                   "Tokens out / episode | Text tool calls |")
        out.append("|---|---:|---:|---:|---:|---:|---:|---:|---:|")
    order = sorted(rows, key=lambda k: -sum(bool(r.get("success")) for r in rows[k] if valid(r)) / max(1, len(
        [r for r in rows[k] if valid(r)])))
    for label in order:
        rs = [r for r in rows[label] if valid(r)]
        n = len(rs)
        if not n:
            continue
        succ = sum(bool(r.get("success")) for r in rs)
        fixed = sum(bool(r.get("resolved")) for r in rs)
        named = sum(bool(r.get("diagnosed")) for r in rs)
        coll = sum(bool(r.get("collateral")) or bool(r.get("blocked_commands")) for r in rs)
        unfinished = sum(r.get("ended") not in ("finished", "oracle", "no-agent") for r in rs)
        walls = sorted((r.get("stats") or {}).get("seconds", r.get("wall_seconds", 0)) / 60 for r in rs)
        med = walls[len(walls) // 2] if walls else None
        steps = mean([(r.get("stats") or {}).get("steps") for r in rs])
        tokens = mean([(r.get("stats") or {}).get("completion_tokens") for r in rs])
        calls = sum((r.get("stats") or {}).get("tool_calls", 0) for r in rs)
        text_calls = sum((r.get("stats") or {}).get("text_tool_calls", 0) for r in rs)
        if track == "diagnostics":
            out.append(f"| {label} | {n} | **{pct(succ, n)}** | {pct(fixed, n)} | {pct(named, n)} | {coll} | "
                       f"{unfinished} | {fmt(med, 1)} | {fmt(steps, 1)} | {fmt(tokens)} | {pct(text_calls, calls)} |")
        else:
            out.append(f"| {label} | {n} | **{pct(succ, n)}** | {coll} | {unfinished} | {fmt(med, 1)} | "
                       f"{fmt(steps, 1)} | {fmt(tokens)} | {pct(text_calls, calls)} |")
    return "\n".join(out)


def valid(r):
    return not r.get("invalid") and not r.get("harness_error")


def matrix(results, track):
    labels = sorted({label_of(r) for r in results if r.get("track") == track})
    scen = sorted({r["scenario"] for r in results if r.get("track") == track})
    cell = defaultdict(list)
    for r in results:
        if r.get("track") == track:
            cell[(r["scenario"], label_of(r))].append(r)
    out = ["| Scenario | " + " | ".join(labels) + " |", "|---|" + "---:|" * len(labels)]
    for s in scen:
        row = []
        for label in labels:
            rs = cell.get((s, label), [])
            marks = []
            for r in sorted(rs, key=lambda r: r.get("repeat", 0)):
                if not valid(r):
                    marks.append("E")
                elif r.get("success"):
                    marks.append("✓" if track == "computer-use" or r.get("diagnosed") else "✓·")
                elif r.get("resolved"):
                    marks.append("✗c")  # fixed, but with collateral damage
                else:
                    marks.append("✗")
            row.append(" ".join(marks) or "")
        out.append(f"| {s} | " + " | ".join(row) + " |")
    return "\n".join(out)


def problems(results):
    bad = [r for r in results if not valid(r)]
    if not bad:
        return "None."
    return "\n".join(f"- {label_of(r)} {r['scenario']} r{r.get('repeat', 0)}: "
                     f"{r.get('invalid') or r.get('harness_error')}" for r in bad)


def main():
    results = load(sys.argv[1:])
    print("## Diagnostics\n")
    print(summary(results, "diagnostics"))
    print("\nSuccess: fixed (the scenario's check passes afterwards) with no collateral damage. Root cause named: "
          "the report matches the scenario's cause patterns. Collateral: episodes that broke an invariant or ran a "
          "refused command. Unfinished: ran out of steps or time, stopped acting, or the server failed.\n")
    print(matrix(results, "diagnostics"))
    print("\n✓ fixed and named the cause · ✓· fixed without naming it · ✗c fixed with collateral damage · "
          "✗ not fixed · E invalid or harness error\n")
    if any(r.get("track") == "computer-use" for r in results):
        print("## Computer use\n")
        print(summary(results, "computer-use"))
        print()
        print(matrix(results, "computer-use"))
        print()
    print("## Excluded episodes\n")
    print(problems(results))


if __name__ == "__main__":
    main()
