"""Keeps a llama-server decoding: short requests one after another until
killed; prints each reply's end time and decode speed."""
import json
import sys
import time
import urllib.request

port = sys.argv[1]
prompt = "Explain, step by step and in detail, how a Linux system boots, from firmware to the login screen."
while True:
    started = time.time()
    body = json.dumps({"prompt": prompt, "n_predict": 64, "temperature": 1.0, "cache_prompt": True}).encode()
    req = urllib.request.Request(f"http://127.0.0.1:{port}/completion", body, {"Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=300) as r:
        t = json.load(r).get("timings", {})
    print(round(started, 1), round(time.time(), 1), t.get("predicted_n", 0), round(t.get("predicted_per_second", 0), 1), flush=True)
