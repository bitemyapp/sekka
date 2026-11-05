"""Turn agent-evaluation episodes into fine-tuning data: rejection-sample
the good ones and write each as an OpenAI-format chat conversation.

    python3 export_sft.py RUN… --out data.jsonl [--images DIR] [--summary summary.md]

An episode is kept when it succeeded (its check passed, nothing else was
harmed), was safe (no refused commands), ended by calling `finish`, and
ran cleanly: every tool call native and well-formed, no nudges, no
server errors. Near-duplicates (the same scenario solved with nearly the
same commands or actions) are dropped, keeping the shortest.

Each line: {"messages": [...], "tools": [...], "metadata": {...}}. The
messages are the ones the model saw and wrote: the system prompt, the
user's words, assistant turns with `tool_calls` (and `reasoning_content`
when the model reasoned), and `tool` results. Computer-use screenshots
are copied beside the data and referenced by relative path in
`image_url` parts; the harness showed the model only the newest three
(`metadata.keep_images`), older ones as "[an earlier screenshot, no
longer shown]".
"""

import argparse
import collections
import difflib
import json
import shutil
import statistics
import sys
from pathlib import Path

NUDGE = "Continue by calling one of the tools. When you are done, call `finish`."


def episodes(runs):
    for run in runs:
        for result in sorted(Path(run).glob("*/*/result.json")):
            try:
                r = json.loads(result.read_text())
            except json.JSONDecodeError:
                continue
            r["dir"] = str(result.parent)
            r["run"] = Path(run).name
            yield r


def why_not(r):
    """Why an episode isn't kept, or None."""
    if r.get("harness_error"):
        return "harness error"
    if r.get("invalid"):
        return "invalid"
    if not r.get("success"):
        return "failed"
    if r.get("collateral") or r.get("blocked_commands"):
        return "unsafe"
    if r.get("ended") != "finished":
        return f"ended {r.get('ended')}"
    s = r.get("stats") or {}
    for k in ("text_tool_calls", "bad_arguments", "unknown_tools", "nudges", "malformed_calls"):
        if s.get(k):
            return k.replace("_", " ")
    if not (Path(r["dir"]) / "transcript.jsonl").exists():
        return "no transcript"
    return None


def conversation(r, images_out, image_prefix):
    """The episode's messages, tools and actions (for comparing episodes)."""
    events = [json.loads(line) for line in open(Path(r["dir"]) / "transcript.jsonl")]
    start = next(e for e in events if e["kind"] == "start")
    shots = sorted(Path(r["dir"]).glob("step-*.png"), key=lambda p: int(p.stem.split("-")[1]))
    shot_iter = iter(shots)
    key = f"{r['run']}/{Path(r['dir']).parent.name}/{Path(r['dir']).name}"

    def image(path):
        rel = f"{image_prefix}/{key}/{path.name}"
        if images_out:
            dest = Path(images_out) / key / path.name
            dest.parent.mkdir(parents=True, exist_ok=True)
            if not dest.exists():
                shutil.copyfile(path, dest)
        return {"type": "image_url", "image_url": {"url": rel}}

    messages = [{"role": "system", "content": start["system"]}]
    if r["track"] == "computer-use":
        first = next(shot_iter, None)
        messages.append({"role": "user", "content": [{"type": "text", "text": start["task"]}]
                         + ([image(first)] if first else [])})
    else:
        messages.append({"role": "user", "content": start["task"]})
    actions = []
    tool_events = collections.deque(e for e in events if e["kind"] == "tool")
    for e in events:
        if e["kind"] != "assistant":
            continue
        step = e["step"]
        calls = []
        for i, c in enumerate(e.get("tool_calls") or []):
            args = c["arguments"]
            calls.append({"id": f"call_{step}_{i}", "type": "function",
                          "function": {"name": c["name"],
                                       "arguments": args if isinstance(args, str) else json.dumps(args)}})
        m = {"role": "assistant", "content": e.get("content") or ""}
        if e.get("reasoning"):
            m["reasoning_content"] = e["reasoning"]
        if calls:
            m["tool_calls"] = calls
        messages.append(m)
        turn_images = []
        for c in calls:
            name = c["function"]["name"]
            if name == "finish":
                messages.append({"role": "tool", "tool_call_id": c["id"], "content": "Finished."})
                continue
            t = tool_events.popleft()
            assert t["step"] == step and t["name"] == name, (r["dir"], step, name, t)
            messages.append({"role": "tool", "tool_call_id": c["id"], "content": t["output"]})
            args = t.get("arguments") or {}
            actions.append(args.get("command") if name == "shell" else json.dumps({"name": name, **args},
                                                                                  sort_keys=True))
            for _ in range(t.get("images") or 0):
                shot = next(shot_iter, None)
                if shot:
                    turn_images.append(shot)
        if turn_images:
            messages.append({"role": "user", "content": [{"type": "text", "text": "The screen now:"},
                                                         image(turn_images[-1])]})
    return messages, start["tools"], actions


def similar(a, b):
    return difflib.SequenceMatcher(None, "\n".join(a), "\n".join(b)).ratio()


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("runs", nargs="+", help="run directories (.work/evals/runs/NAME)")
    ap.add_argument("--out", required=True)
    ap.add_argument("--images", help="where to copy screenshots (default: beside --out, NAME-images)")
    ap.add_argument("--summary", help="write a Markdown summary here")
    ap.add_argument("--dedupe", type=float, default=0.9,
                    help="drop an episode whose actions are at least this similar to a kept one's")
    ap.add_argument("--models", help="comma-separated: keep only these models' episodes (all are summarised)")
    args = ap.parse_args()

    out = Path(args.out)
    images_dir = Path(args.images) if args.images else out.with_name(out.stem + "-images")
    prefix = images_dir.name
    teachers = set(args.models.split(",")) if args.models else None
    all_eps = list(episodes(args.runs))
    reasons = collections.Counter()
    by_model = collections.defaultdict(lambda: collections.Counter())
    candidates = []
    for r in all_eps:
        m = r.get("model") or r.get("agent")
        if not r.get("harness_error") and not r.get("invalid"):
            by_model[(m, r["track"])]["episodes"] += 1
            by_model[(m, r["track"])]["success"] += bool(r.get("success"))
            by_model[(m, r["track"])]["diagnosed"] += bool(r.get("diagnosed"))
        else:
            by_model[(m, r["track"])]["excluded"] += 1
        if teachers and m not in teachers:
            continue
        why = why_not(r)
        if why:
            reasons[why] += 1
            continue
        candidates.append(r)

    # Shortest first, so a duplicate keeps the cheaper solution.
    candidates.sort(key=lambda r: (r["scenario"], (r.get("stats") or {}).get("steps", 0),
                                   (r.get("stats") or {}).get("completion_tokens", 0)))
    kept, per_scenario = [], collections.defaultdict(list)
    out.parent.mkdir(parents=True, exist_ok=True)
    with open(out, "w") as f:
        for r in candidates:
            try:
                messages, tools, actions = conversation(r, None, prefix)
            except (AssertionError, StopIteration, KeyError) as e:
                reasons[f"transcript mismatch ({type(e).__name__})"] += 1
                print(f"skipping {r['dir']}: {e}", file=sys.stderr)
                continue
            if any(similar(actions, other) >= args.dedupe for other in per_scenario[r["scenario"]]):
                reasons["near-duplicate"] += 1
                continue
            messages, tools, actions = conversation(r, images_dir, prefix)
            per_scenario[r["scenario"]].append(actions)
            s = r.get("stats") or {}
            meta = {k: r.get(k) for k in ("scenario", "track", "area", "level", "topic", "model", "repeat", "run",
                                          "diagnosed", "prompt_version", "observe")}
            meta.update({"steps": s.get("steps"), "completion_tokens": s.get("completion_tokens"),
                         "final_prompt_tokens": s.get("max_prompt_tokens"), "seconds": s.get("seconds"),
                         "episode": r["dir"]})
            if r["track"] == "computer-use":
                meta["keep_images"] = 3
            f.write(json.dumps({"messages": messages, "tools": tools, "metadata": meta}, ensure_ascii=False) + "\n")
            kept.append(meta)

    lines = [f"# {out.name}", "", f"{len(kept)} conversations kept from {len(all_eps)} episodes "
             f"({', '.join(Path(r).name for r in args.runs)}).", ""]
    lines += ["## Success on the training set, by model", "", "| Model | Track | Episodes | Succeeded | Named the cause |",
              "|---|---|---:|---:|---:|"]
    for (m, track), c in sorted(by_model.items()):
        n = c["episodes"]
        pct = f" ({100 * c['success'] / n:.0f}%)" if n else ""
        lines.append(f"| {m} | {track} | {n} | {c['success']}{pct} | "
                     f"{c['diagnosed'] if track == 'diagnostics' else '–'} |"
                     + (f" ({c['excluded']} excluded)" if c["excluded"] else ""))
    lines += ["", "## Not kept", ""] + [f"- {why}: {n}" for why, n in reasons.most_common()]
    lines += ["", "## Kept, by scenario", "", "| Scenario | Track | Area | Topic | Kept | Models |", "|---|---|---|---|---:|---|"]
    by_scen = collections.defaultdict(list)
    for k in kept:
        by_scen[k["scenario"]].append(k)
    seen = {r["scenario"]: r for r in all_eps}
    for sc in sorted(seen, key=lambda x: (seen[x]["track"], x)):
        ks = by_scen.get(sc, [])
        models = collections.Counter(k["model"] for k in ks)
        lines.append(f"| {sc} | {seen[sc]['track']} | {seen[sc].get('area')} | {seen[sc].get('topic') or ''} | "
                     f"{len(ks)} | {', '.join(f'{m} {n}' for m, n in models.items())} |")
    if kept:
        comp = [k["completion_tokens"] or 0 for k in kept]
        ctx = [(k["final_prompt_tokens"] or 0) for k in kept]
        lines += ["", "## Tokens", "",
                  f"- Completion (assistant) tokens: {sum(comp):,} in all; median {statistics.median(comp):,.0f} "
                  f"per conversation.",
                  f"- Conversation length (the last request's prompt): median {statistics.median(ctx):,.0f}, "
                  f"largest {max(ctx):,}; about {sum(ctx) + sum(comp):,} tokens in all.",
                  f"- Steps: median {statistics.median([k['steps'] or 0 for k in kept]):.0f}."]
    text = "\n".join(lines) + "\n"
    if args.summary:
        Path(args.summary).write_text(text)
    print(text)


if __name__ == "__main__":
    main()
