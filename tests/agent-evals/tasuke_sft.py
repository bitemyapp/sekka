"""Teacher data in Tasuke's own format: Tasuke episodes turned into
chat-format training examples exactly as Tasuke sent them to the model:
the system prompt and tools captured from Tasuke in the evaluation VM
(tasuke_capture.py), then the session's own messages (the user's words
with Tasuke's environment block, assistant turns with reasoning and tool
calls, tool results).

An episode is kept when it ran on the training set, succeeded, harmed
nothing, had no refused or invalid calls, finished, and wasn't summarized
(so the conversation is the one the model saw). Near-duplicates (the same
scenario with nearly the same calls) are dropped, keeping the shortest,
as export_sft.py does.

    python3 -I tasuke_sft.py CAPTURE.jsonl RUN_DIR... --out data.jsonl
"""

import argparse
import collections
import difflib
import json
from pathlib import Path


def why_not(r, d):
    if json.loads((d / "config.json").read_text()).get("set") != "train":
        return "not the training set"
    if r.get("harness_error") or r.get("invalid"):
        return "harness"
    if not r.get("success"):
        return "failed"
    if r.get("collateral"):
        return "unsafe"
    if r.get("ended") != "finished":
        return f"ended {r.get('ended')}"
    s = r.get("stats") or {}
    if s.get("refused") or s.get("invalid_calls"):
        return "refused or invalid calls"
    if not (d / "tasuke-session.jsonl").exists():
        return "no session"
    return None


def convert(m):
    if m["role"] == "assistant":
        out = {"role": "assistant", "content": m.get("content") or ""}
        if m.get("reasoning"):
            out["reasoning_content"] = m["reasoning"]
        if m.get("tool_calls"):
            out["tool_calls"] = [{"id": c["id"], "type": "function",
                                  "function": {"name": c["name"], "arguments": c["arguments"]}}
                                 for c in m["tool_calls"]]
        return out
    if m["role"] == "tool":
        return {"role": "tool", "content": m["content"], "tool_call_id": m["tool_call_id"]}
    return {"role": m["role"], "content": m["content"]}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("capture")
    ap.add_argument("runs", nargs="+")
    ap.add_argument("--out", required=True)
    ap.add_argument("--dedupe", type=float, default=0.9)
    args = ap.parse_args()
    sent = json.loads(Path(args.capture).read_text().splitlines()[0])["body"]
    system, tools = sent["messages"][0], sent["tools"]
    assert system["role"] == "system"

    reasons, candidates = collections.Counter(), []
    for run in args.runs:
        for result in sorted(Path(run).glob("*/*/result.json")):
            d = result.parent
            r = json.loads(result.read_text())
            why = why_not(r, d)
            if why:
                reasons[why] += 1
                continue
            records = [json.loads(line) for line in (d / "tasuke-session.jsonl").read_text().splitlines() if line.strip()]
            if any(x["kind"] == "summary" for x in records):
                reasons["summarized"] += 1
                continue
            messages = [x["message"] for x in records if x["kind"] == "message"]
            if any(not isinstance(m.get("content"), str) for m in messages if m["role"] != "assistant"):
                reasons["images"] += 1
                continue
            actions = [f"{c['name']} {c['arguments']}" for m in messages for c in m.get("tool_calls") or []]
            candidates.append((r, d, messages, actions))

    candidates.sort(key=lambda c: (c[0]["scenario"], (c[0].get("stats") or {}).get("steps", 0),
                                   (c[0].get("stats") or {}).get("completion_tokens", 0)))
    kept, per_scenario = 0, collections.defaultdict(list)
    with open(args.out, "w") as f:
        for r, d, messages, actions in candidates:
            if any(difflib.SequenceMatcher(None, "\n".join(actions), "\n".join(o)).ratio() >= args.dedupe
                   for o in per_scenario[r["scenario"]]):
                reasons["near-duplicate"] += 1
                continue
            per_scenario[r["scenario"]].append(actions)
            example = {"messages": [system] + [convert(m) for m in messages], "tools": tools,
                       "metadata": {"scenario": r["scenario"], "repeat": r.get("repeat"), "episode": str(d),
                                    "steps": (r.get("stats") or {}).get("steps"),
                                    "followups": r.get("followups", 0)}}
            f.write(json.dumps(example, ensure_ascii=False) + "\n")
            kept += 1
    print(f"exported: {kept} conversations from {len(per_scenario)} scenarios; skipped {dict(reasons)}")


if __name__ == "__main__":
    main()
