"""Download, serve and measure the open models in models.toml on the local
GPU, behind one OpenAI-compatible endpoint.

    sekka-model-server list
    sekka-model-server download qwen3.8-27b
    sekka-model-server start qwen3.8-27b --parallel 4 --ctx 40960
    sekka-model-server bench qwen3.8-27b
    sekka-model-server stop

llama.cpp runs as a local process (nix/local-models.nix supplies
llama-server). vLLM and SGLang run in their pinned upstream containers
under (rootless) Docker, given the host's GPU devices and driver directly,
which works on NixOS without the NVIDIA container toolkit.
"""

import argparse
import json
import os
import signal
import subprocess
import sys
import threading
import time
import tomllib
import urllib.error
import urllib.request
from pathlib import Path

HERE = Path(__file__).resolve().parent
REGISTRY = Path(os.environ.get("SEKKA_MODELS_TOML", HERE / "models.toml"))
CACHE = Path(os.environ.get("SEKKA_MODELS", Path.home() / ".cache" / "sekka-models"))
STATE = CACHE / "server.json"
IMAGES = {
    # Pinned by digest, as inferrous pins them (the images are already
    # pulled on runnymede).
    "vllm": "vllm/vllm-openai@sha256:befccf0e7e4089aa23926bb6f887a85c0d46c2a99e6de5c5ffdf5cb7957c5b34",
    "sglang": "lmsysorg/sglang@sha256:e3a4f06001a29542fc4a8638536d9ffc5eb9c9942b08603c09b19a56eae4a1ae",
}


def registry():
    with open(REGISTRY, "rb") as f:
        return tomllib.load(f)["models"]


def model(name):
    models = registry()
    if name not in models:
        sys.exit(f"no model {name!r} in {REGISTRY}; models: {', '.join(models)}")
    return {"name": name, **models[name]}


def files_of(m):
    return [f for f in (m.get("file"), m.get("mmproj"), m.get("draft")) if f]


def path_of(m, f):
    return CACHE / m["name"] / f


# Downloading.


def download(m):
    if not files_of(m):
        print(f"{m['name']}: the {m['engine']} container downloads {m['repo']} itself on first start")
        return
    for f in files_of(m):
        dest = path_of(m, f)
        if dest.exists():
            print(f"{dest}: present")
            continue
        dest.parent.mkdir(parents=True, exist_ok=True)
        url = f"https://huggingface.co/{m['repo']}/resolve/{m['revision']}/{f}"
        part = dest.with_name(dest.name + ".part")
        print(f"downloading {url}", flush=True)
        # curl resumes (-C -); a dropped connection mid-file gets another go.
        for attempt in range(20):
            done = subprocess.run(["curl", "-fL", "--retry", "8", "--retry-delay", "5", "--retry-all-errors",
                                   "-C", "-", "-o", str(part), url])
            if done.returncode == 0:
                break
            print(f"curl exited {done.returncode}; resuming", flush=True)
            time.sleep(5)
        else:
            sys.exit(f"couldn't download {url}")
        part.rename(dest)
    print(f"{m['name']}: {sum(path_of(m, f).stat().st_size for f in files_of(m)) / 1e9:.1f} GB in {CACHE / m['name']}")


def remove(m):
    for f in files_of(m):
        for p in (path_of(m, f), path_of(m, f).with_name(f + ".part")):
            if p.exists():
                p.unlink()
                print(f"removed {p}")


# Serving.


def driver_dir():
    lib = Path(os.environ.get("NVIDIA_DRIVER_LIBRARY", "/run/opengl-driver/lib/libcuda.so.1"))
    if not lib.exists():
        sys.exit("Set NVIDIA_DRIVER_LIBRARY to the host's libcuda.so.1.")
    return lib.resolve().parent


def container_command(m, port, parallel, ctx, extra):
    engine = m["engine"]
    hf = CACHE / "hf"
    hf.mkdir(parents=True, exist_ok=True)
    security = subprocess.run(["docker", "info", "--format", "{{json .SecurityOptions}}"], capture_output=True,
                              text=True).stdout
    user = "0:0" if "rootless" in security else f"{os.getuid()}:{os.getgid()}"
    base = [
        "docker", "run", "--rm", "--name", f"sekka-model-{port}", "--user", user, "--shm-size", "16g",
        "--device", "/dev/nvidia0", "--device", "/dev/nvidiactl", "--device", "/dev/nvidia-uvm",
        "--volume", f"{driver_dir()}:/usr/local/nvidia/lib64:ro",
        "--volume", f"{hf}:/hf",
        "--env", "LD_LIBRARY_PATH=/usr/local/nvidia/lib64:/usr/local/cuda/lib64",
        "--env", "HF_HOME=/hf", "--env", "HOME=/hf", "--env", "VLLM_NO_USAGE_STATS=1",
        "--env", "PYTORCH_CUDA_ALLOC_CONF=expandable_segments:True",
        "--publish", f"127.0.0.1:{port}:8000",
    ]
    if engine == "vllm":
        return base + ["--entrypoint", "vllm", IMAGES["vllm"], "serve", m["repo"],
                       "--revision", m["revision"], "--served-model-name", m["name"],
                       "--host", "0.0.0.0", "--port", "8000",
                       "--max-model-len", str(ctx), "--max-num-seqs", str(parallel),
                       "--gpu-memory-utilization", os.environ.get("SEKKA_GPU_FRACTION", "0.90"),
                       "--enable-prefix-caching", *m.get("args", []), *extra]
    if engine == "sglang":
        return base + ["--entrypoint", "python3", IMAGES["sglang"], "-m", "sglang.launch_server",
                       "--model-path", m["repo"], "--revision", m["revision"], "--served-model-name", m["name"],
                       "--host", "0.0.0.0", "--port", "8000", "--context-length", str(ctx),
                       "--max-running-requests", str(parallel), "--attention-backend", "flashinfer",
                       *m.get("args", []), *extra]
    raise SystemExit(f"unknown engine {engine}")


def llama_command(m, port, parallel, ctx, extra, total=None, kv="q8_0"):
    server = os.environ.get("SEKKA_LLAMA_SERVER", "llama-server")
    argv = [server, "--model", str(path_of(m, m["file"])), "--alias", m["name"], "--host", "127.0.0.1",
            "--port", str(port), "--jinja", "--n-gpu-layers", "999", "--flash-attn", "on",
            # One KV pool for every slot: a slot may use up to ctx tokens
            # while the total stays within what fits.
            "--ctx-size", str(total or ctx * parallel), "--parallel", str(parallel), "--kv-unified",
            "--kv-unified-per-slot", str(ctx), "--cache-type-k", kv, "--cache-type-v", kv,
            "--metrics", "--no-webui"]
    if m.get("mmproj"):
        argv += ["--mmproj", str(path_of(m, m["mmproj"]))]
    return argv + list(m.get("args", [])) + list(extra)


def health(port):
    try:
        with urllib.request.urlopen(f"http://127.0.0.1:{port}/health", timeout=5) as r:
            return r.status == 200
    except (urllib.error.URLError, OSError):
        return False


def start(m, port, parallel, ctx, extra, wait=1800, total=None, kv="q8_0"):
    stop(quiet=True)
    for f in files_of(m):
        if not path_of(m, f).exists():
            sys.exit(f"{path_of(m, f)} is missing: run `download {m['name']}` first")
    if m["engine"] == "llama.cpp":
        argv = llama_command(m, port, parallel, ctx, extra, total=total or m.get("total_ctx"), kv=kv)
    else:
        argv = container_command(m, port, parallel, ctx, extra)
    log = CACHE / f"server-{m['name']}.log"
    CACHE.mkdir(parents=True, exist_ok=True)
    out = open(log, "w")
    print(" ".join(argv), file=out, flush=True)
    proc = subprocess.Popen(argv, stdout=out, stderr=subprocess.STDOUT, start_new_session=True)
    STATE.write_text(json.dumps({"pid": proc.pid, "model": m["name"], "engine": m["engine"], "port": port,
                                 "parallel": parallel, "ctx": ctx, "argv": argv, "log": str(log),
                                 "started": time.time()}))
    deadline = time.monotonic() + wait
    while time.monotonic() < deadline:
        if proc.poll() is not None:
            sys.exit(f"the server exited ({proc.returncode}); see {log}:\n" + log.read_text()[-3000:])
        if health(port):
            print(f"{m['name']} is up on http://127.0.0.1:{port}/v1 ({m['engine']}, {parallel} slots of {ctx} tokens)")
            return
        time.sleep(2)
    sys.exit(f"the server didn't come up in {wait} s; see {log}")


def stop(quiet=False):
    if not STATE.exists():
        if not quiet:
            print("no server running")
        return
    s = json.loads(STATE.read_text())
    if s["engine"] != "llama.cpp":
        subprocess.run(["docker", "stop", "-t", "20", f"sekka-model-{s['port']}"], capture_output=True)
    try:
        os.killpg(s["pid"], signal.SIGTERM)
        for _ in range(60):
            os.killpg(s["pid"], 0)
            time.sleep(0.5)
        os.killpg(s["pid"], signal.SIGKILL)
    except ProcessLookupError:
        pass
    STATE.unlink(missing_ok=True)
    if not quiet:
        print(f"stopped {s['model']}")


# Measuring.

BENCH_PROMPT = ("Explain, for an experienced Linux administrator, how systemd socket activation works, how it "
                "interacts with service dependencies, and how to debug a socket-activated service that fails to "
                "start. Be thorough and concrete.")


def stream_once(port, name, max_tokens, prompt=BENCH_PROMPT, params=None):
    body = {"model": name, "messages": [{"role": "user", "content": prompt}], "max_tokens": max_tokens,
            "stream": True, "stream_options": {"include_usage": True}, "temperature": 0.0, **(params or {})}
    req = urllib.request.Request(f"http://127.0.0.1:{port}/v1/chat/completions", data=json.dumps(body).encode(),
                                 headers={"Content-Type": "application/json"})
    t0 = time.monotonic()
    first = None
    pieces = 0
    usage = None
    timings = None
    with urllib.request.urlopen(req, timeout=900) as r:
        for raw in r:
            line = raw.decode().strip()
            if not line.startswith("data:") or line == "data: [DONE]":
                continue
            d = json.loads(line[5:])
            usage = d.get("usage") or usage
            timings = d.get("timings") or timings
            for c in d.get("choices") or []:
                delta = c.get("delta") or {}
                if delta.get("content") or delta.get("reasoning_content") or delta.get("reasoning"):
                    pieces += 1
                    if first is None:
                        first = time.monotonic()
    end = time.monotonic()
    n = (usage or {}).get("completion_tokens") or (timings or {}).get("predicted_n") or pieces
    ttft = (first or end) - t0
    decode = (n - 1) / max(end - (first or end), 1e-6)
    if timings and timings.get("predicted_per_second"):
        decode = timings["predicted_per_second"]
    return {"tokens": n, "ttft": round(ttft, 3), "decode_tps": round(decode, 1), "wall": round(end - t0, 2),
            "prompt_tokens": (usage or {}).get("prompt_tokens") or (timings or {}).get("prompt_n")}


def long_context(port, name, tokens=16000):
    """What an agent loop pays each turn: a cold prefill of a long history,
    then the next turn, whose prefix the server should have cached."""
    line = "Nov 04 03:12:44 studio sekka-backup[811]: chunk {} uploaded in 41 ms (attempt 1, 4 MiB)\n"
    history = "".join(line.format(i) for i in range(tokens // 30))
    prompt = "Here is a journal excerpt:\n" + history + "\nWhich chunk number comes last? Answer with the number."
    cold = stream_once(port, name, 16, prompt=prompt)
    warm = stream_once(port, name, 16, prompt=prompt + " Then say done.")
    n = cold.get("prompt_tokens") or tokens
    return {"prompt_tokens": n, "cold_ttft": cold["ttft"], "cold_prefill_tps": round(n / max(cold["ttft"], 1e-6)),
            "cached_ttft": warm["ttft"]}


def bench(port, name, max_tokens=512, concurrency=(1, 4)):
    stream_once(port, name, 32)  # warm up
    results = {"long_context": long_context(port, name)}
    for c in concurrency:
        runs = []

        def one():
            runs.append(stream_once(port, name, max_tokens))

        t0 = time.monotonic()
        threads = [threading.Thread(target=one) for _ in range(c)]
        for t in threads:
            t.start()
        for t in threads:
            t.join()
        wall = time.monotonic() - t0
        results[f"concurrency_{c}"] = {
            "per_stream_decode_tps": round(sum(r["decode_tps"] for r in runs) / len(runs), 1),
            "aggregate_tps": round(sum(r["tokens"] for r in runs) / wall, 1),
            "ttft": round(sum(r["ttft"] for r in runs) / len(runs), 3),
        }
    return results


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    sub.add_parser("list")
    for c in ("download", "remove"):
        sub.add_parser(c).add_argument("models", nargs="+")
    s = sub.add_parser("start")
    s.add_argument("model")
    s.add_argument("--port", type=int, default=8080)
    s.add_argument("--parallel", type=int, default=4, help="concurrent requests (agent episodes)")
    s.add_argument("--ctx", type=int, default=40960, help="context per request, in tokens")
    s.add_argument("--total-ctx", type=int, help="llama.cpp: tokens of KV for all slots together "
                   "(default: the model's total_ctx, else parallel × ctx)")
    s.add_argument("--kv", default="q8_0", help="llama.cpp KV cache type (q8_0 halves F16's memory)")
    s.add_argument("extra", nargs="*", help="further server arguments, after --")
    sub.add_parser("stop")
    b = sub.add_parser("bench")
    b.add_argument("model", nargs="?")
    b.add_argument("--port", type=int, default=8080)
    b.add_argument("--max-tokens", type=int, default=512)
    b.add_argument("--concurrency", default="1,4")
    a = ap.parse_args()
    if a.cmd == "list":
        for name, m in registry().items():
            have = all(path_of({"name": name}, f).exists() for f in files_of(m)) and files_of(m)
            print(f"{name:24} {m['engine']:10} {m.get('size_gb', 0):5.1f} GB  vision={str(m.get('vision')).lower():5} "
                  f"{m.get('license', ''):14} {'downloaded' if have else ''}")
    elif a.cmd == "download":
        for name in a.models:
            download(model(name))
    elif a.cmd == "remove":
        for name in a.models:
            remove(model(name))
    elif a.cmd == "start":
        start(model(a.model), a.port, a.parallel, a.ctx, a.extra, total=a.total_ctx, kv=a.kv)
    elif a.cmd == "stop":
        stop()
    elif a.cmd == "bench":
        name = a.model or json.loads(STATE.read_text())["model"]
        print(json.dumps(bench(a.port, name, a.max_tokens, [int(c) for c in a.concurrency.split(",")]), indent=2))


if __name__ == "__main__":
    main()
