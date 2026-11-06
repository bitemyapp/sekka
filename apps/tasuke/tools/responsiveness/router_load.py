"""Keeps Tasuke's router busy with MODEL until killed: decoding 256 tokens a
reply, or (prefill) reading a 10K-token prompt and writing 16."""
import json
import sys
import urllib.request
from pathlib import Path

model, mode, key = sys.argv[1], sys.argv[2], Path(sys.argv[3]).read_text().strip()
para = ("Sekka is a desktop Linux distribution built on NixOS. Aizome is its desktop: a compositor, a shell, "
        "and apps that share one toolkit. Tasuke is its local assistant. ")
text = "Summarise this in one line.\n\n" + para * 260 if mode == "prefill" else "Explain, step by step, how a Linux system boots."
while True:
    body = json.dumps({"model": model, "messages": [{"role": "user", "content": text}],
                       "max_tokens": 16 if mode == "prefill" else 256, "cache_prompt": False}).encode()
    req = urllib.request.Request("http://127.0.0.1:8737/v1/chat/completions", body,
                                 {"Content-Type": "application/json", "Authorization": f"Bearer {key}"})
    with urllib.request.urlopen(req, timeout=600) as r:
        t = json.load(r).get("timings", {})
    print(t.get("prompt_n"), round(t.get("prompt_per_second", 0)), round(t.get("predicted_per_second", 0), 1), flush=True)
