"""The 8 GiB round's single-user servers, measured: each model as a person
with an 8 GiB GPU would run it (its `args_8gib`, one slot, 32K context,
q8_0 KV), its peak GPU memory with the context full and, for vision
models, an image decoded, and its decode speed.

    fit8gib.py gemma-4-12b qwen3.5-9b … [--port 8099] [--image PNG]

Prints one JSON line per model. Speeds are on whatever GPU this runs on
(the evaluations' RTX 5090 or RTX PRO 6000), not on an 8 GiB card, whose
memory bandwidth is several times lower; peak memory is what carries over.
"""

import argparse
import base64
import json
import os
import statistics
import subprocess
import sys
import threading
import time
import tomllib
import urllib.request
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import serve  # noqa: E402

LIMIT_MIB = 8192


def post(port, body, timeout=600):
    req = urllib.request.Request(f"http://127.0.0.1:{port}/v1/chat/completions", data=json.dumps(body).encode(),
                                 headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=timeout) as r:
        return json.load(r)


def gpu_mib(pid):
    """This process's GPU memory, MiB (0 when it has none yet)."""
    out = subprocess.run(["nvidia-smi", "--query-compute-apps=pid,used_memory", "--format=csv,noheader,nounits"],
                         capture_output=True, text=True).stdout
    for line in out.splitlines():
        p, _, mib = line.partition(",")
        if p.strip() == str(pid):
            return int(mib)
    return 0


def server_pid(port):
    out = subprocess.run(["pgrep", "-f", f"llama-server .*--port {port} "], capture_output=True, text=True).stdout
    pids = [int(p) for p in out.split()]
    return pids[0] if pids else None


def measure(name, m, port, image):
    extra = list(m.get("args_8gib", []))
    serve.start(m, port, 1, 32768, extra, total=32768)
    pid = server_pid(port)
    peak = [gpu_mib(pid)]
    stop = threading.Event()

    def poll():
        while not stop.is_set():
            peak.append(gpu_mib(pid))
            time.sleep(0.2)

    t = threading.Thread(target=poll, daemon=True)
    t.start()
    result = {"model": name, "args_8gib": extra, "loaded_mib": peak[0]}
    try:
        # The context mostly full: ~25K tokens of short log lines (~11 each).
        filler = "".join(f"line {i} unit-{i % 97} state={'ok' if i % 3 else 'retry'}\n" for i in range(2300))
        r = post(port, {"model": name, "max_tokens": 64, "temperature": 0,
                        "messages": [{"role": "user", "content": filler + "\nHow many lines say retry? Answer briefly."}]})
        result["full_prompt_tokens"] = r.get("usage", {}).get("prompt_tokens")
        if m.get("vision") and image:
            data = base64.b64encode(Path(image).read_bytes()).decode()
            post(port, {"model": name, "max_tokens": 32, "temperature": 0, "messages": [{"role": "user", "content": [
                {"type": "text", "text": "What is on this screen? One sentence."},
                {"type": "image_url", "image_url": {"url": "data:image/png;base64," + data}}]}]})
        speeds = []
        for _ in range(3):
            r = post(port, {"model": name, "max_tokens": 256, "temperature": 0, "messages": [
                {"role": "user", "content": "Explain, in about 200 words, what a systemd user service is."}]})
            tm = r.get("timings") or {}
            if tm.get("predicted_per_second"):
                speeds.append(tm["predicted_per_second"])
        result["decode_tps"] = round(statistics.median(speeds), 1) if speeds else None
    finally:
        stop.set()
        t.join()
        serve.stop(quiet=True, port=port)
    result["peak_mib"] = max(peak)
    result["fits_8gib"] = result["peak_mib"] <= LIMIT_MIB
    return result


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("models", nargs="+")
    ap.add_argument("--port", type=int, default=8099)
    ap.add_argument("--image", help="a screenshot for vision models")
    args = ap.parse_args()
    with open(HERE / "models.toml", "rb") as f:
        registry = tomllib.load(f)["models"]
    for name in args.models:
        m = dict(registry[name], name=name)
        try:
            print(json.dumps(measure(name, m, args.port, args.image)), flush=True)
        except Exception as e:  # noqa: BLE001 (one model's failure is reported, the rest still run)
            serve.stop(quiet=True, port=args.port)
            print(json.dumps({"model": name, "error": f"{type(e).__name__}: {e}"}), flush=True)


if __name__ == "__main__":
    main()
