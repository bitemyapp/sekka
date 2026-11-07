"""Print what each episode did, for reading results by hand: the outcome,
the agent's report, and the commands that changed the machine.

    python3 review.py .work/evals/runs/RUN [--model NAME] [--scenario ID] [--failed]
"""

import argparse
import json
import re
from pathlib import Path

CHANGES = re.compile(
    r"\b(chmod|chown|sed\s+-i|tee\b|truncate|rm\s|mv\s|cp\s|kill|pkill|killall|nft\s+(add|delete|flush)|iptables\s+-[ADFI]|"
    r"systemctl\s+(--user\s+)?(start|stop|restart|reload|mask|unmask|enable|disable|reset-failed|daemon-reload)|"
    r"timedatectl\s+set|resolvconf\s+-u|install\s|ln\s)"
    # A redirection into a file (not 2>, not /dev/null).
    r"|(?<![0-9&>])>{1,2}\s*(?!/dev/null|&)[\w~/$\"']")


def episodes(root):
    for result in sorted(Path(root).glob("**/result.json")):
        yield result.parent, json.loads(result.read_text())


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("runs", nargs="+")
    ap.add_argument("--model")
    ap.add_argument("--scenario")
    ap.add_argument("--failed", action="store_true", help="only episodes that didn't succeed")
    a = ap.parse_args()
    for root in a.runs:
        for d, r in episodes(root):
            if a.model and r.get("model") != a.model:
                continue
            if a.scenario and r["scenario"] != a.scenario:
                continue
            if a.failed and r.get("success"):
                continue
            stats = r.get("stats") or {}
            print(f"## {r.get('model') or r.get('agent')} · {r['scenario']} r{r.get('repeat', 0)}"
                  f"{' · skill' if r.get('skill') else ''}{' · ' + r['observe'] if r.get('track') == 'computer-use' else ''}")
            print(f"success={r.get('success')} resolved={r.get('resolved')} diagnosed={r.get('diagnosed')} "
                  f"collateral={r.get('collateral')} ended={r.get('ended')} steps={stats.get('steps')} "
                  f"seconds={stats.get('seconds')} tokens_out={stats.get('completion_tokens')} "
                  f"{r.get('invalid') or r.get('harness_error') or ''}")
            print(f"check: {json.dumps(r.get('check'))[:300]}")
            final = r.get("final") or {}
            for k in ("root_cause", "changes", "answer", "text"):
                if final.get(k):
                    print(f"{k}: {str(final[k])[:700]}")
            t = d / "transcript.jsonl"
            if t.exists():
                for line in t.read_text().splitlines():
                    e = json.loads(line)
                    if e["kind"] == "tool" and e["name"] == "shell":
                        cmd = e["arguments"].get("command", "")
                        if CHANGES.search(cmd):
                            print(f"  $ {cmd[:240].replace(chr(10), ' ⏎ ')}")
                    elif e["kind"] == "llm-error":
                        print(f"  ! {e['error'][:200]}")
            if r.get("blocked_commands"):
                print(f"refused: {r['blocked_commands']}")
            print()


if __name__ == "__main__":
    main()
