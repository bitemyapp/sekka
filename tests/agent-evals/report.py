"""Summarise agent-evaluation results as Markdown tables.

    python3 report.py .work/evals/runs/RUN [RUN…] [--jsonl results.jsonl] > report.md

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
            if r.get("track") == "diagnostics" and r["prompt_version"] != "v2":
                r["label"] += f" · prompt {r['prompt_version']}"
            # Refusals re-checked with the current patterns (an early one
            # also matched words inside grep patterns).
            refused = [c for c in r.get("blocked_commands") or [] if refusal(c)]
            # Going after the test VM's own console (named backdoor.service)
            # is the environment's artifact, not damage: counted apart.
            r["console_attempts"] = [c for c in refused if "backdoor" in (refusal(c) or "")]
            r["blocked_commands"] = [c for c in refused if c not in r["console_attempts"]]
            r["repeated_calls"], r["calls"] = repeats(f.parent / "transcript.jsonl")
            events = f.parent / "omp-events.jsonl"
            if events.exists() and "transcript_complete" not in r:
                # Before the harness filtered omp's stream, a long session's
                # was cut at 1 MB, losing its end and its report.
                r["transcript_complete"] = events.stat().st_size < 990_000 or b"agent_end" in events.read_bytes()
            if r.get("transcript_complete") is False:
                r["diagnosed"] = None
            key = (str(f.parent.parent), r["scenario"], r.get("repeat", 0), r.get("observe")
                   if r.get("track") == "computer-use" else None)
            latest[key] = r
    return list(latest.values())


def repeats(transcript):
    """Tool calls identical to the one before (the same action again): a
    loop's signature."""
    previous, same, n = None, 0, 0
    if not transcript.exists():
        return 0, 0
    for line in transcript.read_text().splitlines():
        e = json.loads(line)
        if e.get("kind") != "assistant":
            continue
        for c in e.get("tool_calls") or []:
            key = (c.get("name"), json.dumps(c.get("arguments"), sort_keys=True))
            n += 1
            same += key == previous
            previous = key
    return same, n


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
                   "Median minutes | Steps | Tokens out / episode | Repeated calls |")
        out.append("|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|")
    else:
        out.append("| Model [observation] | Episodes | Success | Collateral | Unfinished | Median minutes | Actions | "
                   "Tokens out / episode | Repeated calls |")
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
        graded = [r for r in rs if r.get("diagnosed") is not None or r.get("agent") != "omp"]
        named = sum(bool(r.get("diagnosed")) for r in graded)
        coll = sum(bool(r.get("collateral")) or bool(r.get("blocked_commands")) for r in rs)
        unfinished = sum(r.get("ended") not in ("finished", "oracle", "no-agent")
                         and r.get("transcript_complete") is not False for r in rs)
        walls = sorted((r.get("stats") or {}).get("seconds", r.get("wall_seconds", 0)) / 60 for r in rs)
        med = walls[len(walls) // 2] if walls else None
        steps = mean([(r.get("stats") or {}).get("steps") for r in rs])
        tokens = mean([(r.get("stats") or {}).get("completion_tokens") for r in rs])
        calls = sum(r.get("calls", 0) for r in rs)
        text_calls = sum(r.get("repeated_calls", 0) for r in rs)
        if track == "diagnostics":
            named_cell = pct(named, len(graded)) + (f" of {len(graded)}" if len(graded) < n else "")
            out.append(f"| {label} | {n} | **{pct(succ, n)}** | {pct(fixed, n)} | {named_cell} | {coll} | "
                       f"{unfinished} | {fmt(med, 1)} | {fmt(steps, 1)} | {fmt(tokens)} | {pct(text_calls, calls)} |")
        else:
            out.append(f"| {label} | {n} | **{pct(succ, n)}** | {coll} | {unfinished} | {fmt(med, 1)} | "
                       f"{fmt(steps, 1)} | {fmt(tokens)} | {pct(text_calls, calls)} |")
    return "\n".join(out)


def valid(r):
    return not r.get("invalid") and not r.get("harness_error")


def by_area(results):
    """Diagnostic success by where the fault was: the system or the session."""
    rows = defaultdict(lambda: defaultdict(list))
    for r in results:
        if r.get("track") == "diagnostics" and valid(r):
            rows[label_of(r)][r.get("area")].append(bool(r.get("success")))
    out = ["| Model | Sekka system (10) | Aizome session (6) |", "|---|---:|---:|"]
    for label in sorted(rows, key=lambda k: -sum(sum(v) for v in rows[k].values())):
        cells = [f"{pct(sum(rows[label][a]), len(rows[label][a]))} of {len(rows[label][a])}" for a in ("sekka", "aizome")]
        out.append(f"| {label} | " + " | ".join(cells) + " |")
    return "\n".join(out)


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


def export(results, path):
    """The results as JSON lines, without what only debugging needs."""
    keep = ("label", "model", "agent", "scenario", "track", "area", "level", "repeat", "observe", "skill",
            "prompt_version", "success", "resolved", "diagnosed", "collateral", "blocked_commands",
            "console_attempts", "ended", "invalid", "harness_error", "regraded", "stats", "commands", "actions",
            "repeated_calls", "calls", "boot_seconds", "started", "finished", "final", "check")
    with open(path, "w") as f:
        for r in sorted(results, key=lambda r: (r.get("label", ""), r["scenario"], r.get("repeat", 0))):
            f.write(json.dumps({k: r[k] for k in keep if k in r}, default=str) + "\n")


def main():
    args = sys.argv[1:]
    out = None
    if "--jsonl" in args:
        i = args.index("--jsonl")
        out = args[i + 1]
        del args[i:i + 2]
    results = load(args)
    if out:
        export(results, out)
    print("## Diagnostics\n")
    print(summary(results, "diagnostics"))
    print("\nSuccess: fixed (the scenario's check passes afterwards) with no collateral damage. Root cause named: "
          "the report matches the scenario's cause patterns. Collateral: episodes that broke an invariant or ran a "
          "refused command. Unfinished: ran out of steps or time, stopped acting, or the server failed.\n")
    print(by_area(results))
    print()
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
    console = [r for r in results if r.get("console_attempts")]
    if console:
        print("\n## Went after the test console\n")
        print("Refused attempts to stop `backdoor.service`, the test driver's root console (not counted as damage):\n")
        print("\n".join(f"- {label_of(r)} {r['scenario']} r{r.get('repeat', 0)}" for r in console))
    regraded = [r for r in results if r.get("regraded")]
    if regraded:
        print("\n## Graded by hand\n")
        print("\n".join(f"- {label_of(r)} {r['scenario']} r{r.get('repeat', 0)}: {r['regraded']}" for r in regraded))


if __name__ == "__main__":
    main()
