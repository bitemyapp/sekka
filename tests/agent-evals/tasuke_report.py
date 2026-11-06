"""Summaries of `run.py --agent tasuke` runs: per model and track, how many
episodes succeeded, how many on the first turn (before the harness's one
"Yes, please go ahead."), damage, how episodes ended, and the cost.

    tasuke_report.py RUN_DIR... [--by-scenario]
"""

import argparse
import collections
import json
import statistics
from pathlib import Path


def load(dirs):
    """Every episode, a later run's replacing an earlier one's with the same
    label, scenario and repeat (a rerun on a fixed build)."""
    by = {}
    replaced = []
    for d in dirs:
        f = Path(d) / "results.jsonl"
        if not f.exists():
            continue
        for line in f.open():
            r = json.loads(line)
            key = (r.get("label"), r.get("scenario"), r.get("repeat"), r.get("observe"))
            if key in by and Path(d).name != Path(by[key]["_run"]).name:
                replaced.append(key)
            r["_run"] = d
            by[key] = r
    if replaced:
        print(f"replaced by a later run: {sorted(set(replaced))}")
    return list(by.values())


DESKTOP = {"click", "double_click", "drag", "scroll", "type_text", "key", "open_app", "click_element", "type_into"}
COMMANDS = {"run_command", "run_as_root", "write_file", "edit_file"}


def way(r):
    """How a desktop task was done: through the screen, by commands or files, or both."""
    tag = f"{r['scenario']}-r{r.get('repeat', 0)}-{r.get('observe')}"
    session = Path(r["_run"]) / r["label"] / tag / "tasuke-session.jsonl"
    tools = set()
    if session.exists():
        for line in session.open():
            try:
                e = json.loads(line).get("event") or {}
            except json.JSONDecodeError:
                continue
            if e.get("type") == "tool_call":
                tools.add((e.get("call") or {}).get("tool"))
    screen, commands = bool(tools & DESKTOP), bool(tools & COMMANDS)
    return "both" if screen and commands else ("screen" if screen else ("commands" if commands else "neither"))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("runs", nargs="+")
    ap.add_argument("--by-scenario", action="store_true")
    args = ap.parse_args()
    rows = load(args.runs)
    # The test driver's shell can lose its framing ("Invalid base64-encoded
    # string"): the harness then sees no tool calls and every invariant at
    # once as broken, so the episode measured nothing. Counted apart, like
    # harness errors.
    def lost(r):
        st = r.get("stats") or {}
        return (str(r.get("ended", "")).startswith("exit") and not st.get("tool_calls")
                and "session" in (r.get("collateral") or []))

    broken = [r for r in rows if r.get("harness_error") or r.get("invalid") or lost(r)]
    rows = [r for r in rows if r not in broken]
    if broken:
        print(f"excluded (harness failures): {[(r['label'], r['scenario'], r.get('repeat')) for r in broken]}")
    groups = collections.defaultdict(list)
    for r in rows:
        groups[(r["label"], r["track"])].append(r)
    for (label, track), rs in sorted(groups.items()):
        ok = sum(bool(r.get("success")) for r in rs)
        first = sum(bool(r["first_turn"]["resolved"]) if r.get("first_turn") else bool(r.get("success")) for r in rs)
        damaged = [r["scenario"] for r in rs if r.get("collateral")]
        ends = collections.Counter(r.get("ended") for r in rs)
        st = [r.get("stats") or {} for r in rs]
        secs = statistics.median(s.get("seconds", 0) for s in st)
        tools = statistics.median(s.get("tool_calls", 0) for s in st)
        out = statistics.median(s.get("completion_tokens", 0) for s in st)
        tps = [s["decode_tps"] for s in st if s.get("decode_tps")]
        print(f"{label:34} {track:13} {ok:2}/{len(rs):<2} first turn {first:2}  damage {len(damaged)} {damaged or ''}")
        if track == "computer-use":
            how = collections.Counter(way(r) for r in rs if r.get("success"))
            print(f"{'':34} done through the screen {how['screen']}, by commands or files {how['commands']}, both {how['both']}")
        print(f"{'':34} median {secs:.0f} s, {tools:.0f} tool calls, {out:.0f} output tokens"
              f"{f', {statistics.median(tps):.0f} tok/s' if tps else ''}; ended {dict(ends)}")
        if args.by_scenario:
            by = collections.defaultdict(list)
            for r in rs:
                by[r["scenario"]].append(bool(r.get("success")))
            print("   ", "  ".join(f"{s} {sum(v)}/{len(v)}" for s, v in sorted(by.items())))


if __name__ == "__main__":
    main()
