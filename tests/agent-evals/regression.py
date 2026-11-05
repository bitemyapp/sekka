"""A general-capability check for the LoRA round: did a light nudge toward
Sekka's diagnostics cost the model ordinary reasoning? Runs a fixed sample
of GSM8K's test set (grade-school word problems with one numeric answer)
against an OpenAI-compatible server and scores exact match on the final
number. Compare a base model and the same model with an adapter on the
same server settings; a drop of more than a few points means the adapter
pushed too hard.

    regression.py --base-url http://127.0.0.1:8090/v1 --model gemma-4-12b \\
        --data gsm8k-test.jsonl [--n 100] [--seed 7] [--out results.jsonl]

`--data` is GSM8K's test split as JSONL ({"question", "answer"} with the
answer's last line "#### N").
"""

import argparse
import concurrent.futures as cf
import json
import random
import re
import urllib.request

PROMPT = ("Solve this problem. Think step by step, then give the final answer on its own last line as "
          "\"Answer: N\" with just the number.\n\n{q}")


def final_number(text):
    m = re.findall(r"Answer:\s*\$?(-?[\d,]*\.?\d+)", text or "")
    if not m:
        m = re.findall(r"(-?[\d,]*\.?\d+)", text or "")
    if not m:
        return None
    v = m[-1].replace(",", "")
    try:
        f = float(v)
    except ValueError:
        return None
    return int(f) if f == int(f) else f


def ask(base_url, model, question, max_tokens, thinking):
    body = {"model": model, "temperature": 0, "max_tokens": max_tokens,
            "chat_template_kwargs": {"enable_thinking": thinking},
            "messages": [{"role": "user", "content": PROMPT.format(q=question)}]}
    req = urllib.request.Request(base_url.rstrip("/") + "/chat/completions", data=json.dumps(body).encode(),
                                 headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=600) as r:
        reply = json.load(r)
    return reply["choices"][0]["message"].get("content") or ""


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--base-url", required=True)
    ap.add_argument("--model", required=True)
    ap.add_argument("--data", required=True)
    ap.add_argument("--n", type=int, default=100)
    ap.add_argument("--seed", type=int, default=7)
    ap.add_argument("--parallel", type=int, default=8)
    ap.add_argument("--max-tokens", type=int, default=2048, help="without thinking (with it, at least 16384)")
    ap.add_argument("--thinking", choices=["on", "off", "both"], default="both")
    ap.add_argument("--out")
    args = ap.parse_args()
    items = [json.loads(line) for line in open(args.data)]
    random.Random(args.seed).shuffle(items)
    items = items[: args.n]

    def one(item, thinking):
        want = final_number(item["answer"].split("####")[-1])
        limit = max(args.max_tokens, 16384) if thinking else args.max_tokens
        try:
            text = ask(args.base_url, args.model, item["question"], limit, thinking)
        except Exception as e:  # noqa: BLE001 (a failed request counts as wrong, and is recorded)
            text = f"ERROR {e}"
        got = final_number(text)
        return {"question": item["question"][:120], "thinking": thinking, "want": want, "got": got,
                "correct": got == want, "tail": text[-200:]}

    # Thinking can use up a small token limit before any answer, which would
    # score the limit rather than the reasoning: both modes are run, and
    # unanswered items are counted apart.
    modes = {"on": [True], "off": [False], "both": [False, True]}[args.thinking]
    all_results = []
    for thinking in modes:
        with cf.ThreadPoolExecutor(args.parallel) as pool:
            results = list(pool.map(lambda it: one(it, thinking), items))
        all_results += results
        correct = sum(r["correct"] for r in results)
        answered = sum(r["got"] is not None for r in results)
        print(json.dumps({"model": args.model, "thinking": thinking, "n": len(results), "answered": answered,
                          "correct": correct, "accuracy": round(correct / max(1, len(results)), 3)}), flush=True)
    if args.out:
        with open(args.out, "w") as f:
            for r in all_results:
                f.write(json.dumps(r) + "\n")


if __name__ == "__main__":
    main()
