"""Run agent-evaluation episodes: each a fresh VM (the NixOS test driver of
tests/agent-evals), several at once against one local model server.

    # Validate the scenarios: broken they fail, fixed they pass.
    sekka-agent-evals --agent none --scenarios all
    sekka-agent-evals --agent oracle --track diagnostics
    # Evaluate models (each started in turn with sekka-model-server).
    sekka-agent-evals --models qwen3.8-27b,gpt-oss-20b --track diagnostics --repeats 2
    # Against a server you started yourself:
    sekka-agent-evals --models qwen3.8-27b --base-url http://127.0.0.1:8080/v1 --no-serve

Results go to OUT/<run>/<model>/<scenario>-r<n>/ (result.json, transcript.jsonl,
screenshots, the driver's log) and OUT/<run>/results.jsonl; report.py
summarises them.
"""

import argparse
import concurrent.futures as cf
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
import tomllib
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent
HARNESS = Path(os.environ.get("SEKKA_EVAL_HARNESS_DIR", HERE / "harness"))
sys.path.insert(0, str(HARNESS))
import scenarios  # noqa: E402

MODELS_TOML = Path(os.environ.get("SEKKA_MODELS_TOML", HERE / "models.toml"))
SKILL = Path(os.environ.get("SEKKA_EVAL_SKILL", REPO / ".agents/skills/sekka-troubleshooting/SKILL.md"))
SERVER = os.environ.get("SEKKA_MODEL_SERVER")


def server_cmd():
    if SERVER:
        return [SERVER]
    return [sys.executable, "-I", str(HERE / "serve.py")]


def driver():
    d = os.environ.get("SEKKA_EVAL_DRIVER")
    if d:
        return d
    link = REPO / ".work/evals/driver"
    subprocess.run(["nix", "build", "--no-update-lock-file", f"{REPO}#agent-evals.driver", "-o", str(link)],
                   check=True)
    return str(link / "bin/nixos-test-driver")


def episode(drv, cfg, out, timeout):
    out.mkdir(parents=True, exist_ok=True)
    (out / "config.json").write_text(json.dumps(cfg, indent=2))
    # Each driver gets its own short runtime directory: its VLAN socket and
    # VM state live there (UNIX socket paths are limited to 108 bytes).
    rt = tempfile.mkdtemp(prefix="se-", dir=os.environ.get("SEKKA_EVAL_RUNTIME", "/tmp"))
    env = {**os.environ, "XDG_RUNTIME_DIR": rt, "TMPDIR": rt, "SEKKA_EVAL_CONFIG": str(out / "config.json")}
    if os.environ.get("SEKKA_EVAL_LIVE_HARNESS"):
        env["SEKKA_EVAL_HARNESS"] = str(HARNESS)
    t0 = time.monotonic()
    with open(out / "driver.log", "w") as log:
        try:
            subprocess.run([drv, "-o", str(out)], env=env, stdout=log, stderr=subprocess.STDOUT, timeout=timeout,
                           start_new_session=True)
        except subprocess.TimeoutExpired:
            pass
    shutil.rmtree(rt, ignore_errors=True)
    try:
        r = json.loads((out / "result.json").read_text())
    except (FileNotFoundError, json.JSONDecodeError):
        r = {"scenario": cfg["scenario"], "agent": cfg["agent"], "model": (cfg.get("model") or {}).get("name"),
             "harness_error": "no result (driver timed out or crashed); see driver.log"}
    r["wall_seconds"] = round(time.monotonic() - t0, 1)
    r["dir"] = str(out)
    return r


def selected(args):
    chosen = list(scenarios.SCENARIOS.values())
    if args.track:
        chosen = [s for s in chosen if s.track == args.track]
    if args.area:
        chosen = [s for s in chosen if s.area == args.area]
    if args.scenarios and args.scenarios != "all":
        want = args.scenarios.split(",")
        chosen = [s for s in chosen if s.id in want]
    if args.agent == "oracle":
        chosen = [s for s in chosen if s.oracle]
    return chosen


def run_batch(args, drv, run_dir, label, model_cfg, chosen, results):
    jobs = []
    for r in range(args.repeats):
        for s in chosen:
            if s.track == "computer-use" and model_cfg and not model_cfg.get("vision") and args.observe == "screenshot":
                continue
            observe = args.observe or ("screenshot" if (model_cfg or {}).get("vision") else "elements")
            cfg = {"scenario": s.id, "agent": args.agent, "repeat": r, "observe": observe,
                   "max_steps": args.max_steps or (40 if s.track == "diagnostics" else 35),
                   "max_seconds": args.max_seconds}
            if model_cfg:
                cfg["model"] = model_cfg
            if args.skill and s.track == "diagnostics":
                cfg["skill"] = SKILL.read_text()
            tag = f"{s.id}-r{r}" + ("-skill" if cfg.get("skill") else "") + (f"-{observe}" if s.track == "computer-use" else "")
            out = run_dir / label / tag
            if args.resume and (out / "result.json").exists():
                prev = json.loads((out / "result.json").read_text())
                if not prev.get("harness_error"):
                    continue
            cfg["out"] = str(out)
            jobs.append((cfg, out))
    print(f"{label}: {len(jobs)} episodes, {args.parallel} at a time", flush=True)
    with cf.ThreadPoolExecutor(args.parallel) as pool:
        futures = {pool.submit(episode, drv, cfg, out, args.max_seconds + 900): cfg for cfg, out in jobs}
        for f in cf.as_completed(futures):
            r = f.result()
            r["label"] = label
            results.append(r)
            with open(run_dir / "results.jsonl", "a") as fh:
                fh.write(json.dumps(r, default=str) + "\n")
            mark = "ERR " if r.get("harness_error") else ("pass" if r.get("success") else "fail")
            print(f"  {mark} {label} {r['scenario']} r{r.get('repeat')} resolved={r.get('resolved')} "
                  f"diagnosed={r.get('diagnosed')} collateral={r.get('collateral')} ended={r.get('ended')} "
                  f"{r.get('wall_seconds')}s {r.get('harness_error') or r.get('invalid') or ''}", flush=True)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--agent", choices=["model", "none", "oracle"], default="model")
    ap.add_argument("--models", default="", help="comma-separated names from models.toml")
    ap.add_argument("--scenarios", default="all")
    ap.add_argument("--track", choices=["diagnostics", "computer-use"])
    ap.add_argument("--area", choices=["sekka", "aizome"])
    ap.add_argument("--repeats", type=int, default=1)
    ap.add_argument("--parallel", type=int, default=4)
    ap.add_argument("--observe", choices=["screenshot", "elements", "both"])
    ap.add_argument("--skill", action="store_true", help="give diagnostics the troubleshooting skill")
    ap.add_argument("--max-steps", type=int)
    ap.add_argument("--max-seconds", type=int, default=1200)
    ap.add_argument("--out", default=str(REPO / ".work/evals/runs"))
    ap.add_argument("--run", help="run name (default: a timestamp)")
    ap.add_argument("--resume", action="store_true", help="skip episodes with a result")
    ap.add_argument("--base-url", help="an OpenAI-compatible endpoint already serving the model")
    ap.add_argument("--no-serve", action="store_true")
    ap.add_argument("--port", type=int, default=8080)
    ap.add_argument("--ctx", type=int, default=49152, help="context per episode, in tokens")
    args = ap.parse_args()

    run_dir = Path(args.out) / (args.run or time.strftime("%Y%m%d-%H%M%S"))
    run_dir.mkdir(parents=True, exist_ok=True)
    chosen = selected(args)
    drv = driver()
    results = []
    if args.agent != "model":
        run_batch(args, drv, run_dir, args.agent, None, chosen, results)
        return
    with open(MODELS_TOML, "rb") as f:
        registry = tomllib.load(f)["models"]
    for name in [m for m in args.models.split(",") if m]:
        m = registry[name]
        base = args.base_url or f"http://127.0.0.1:{args.port}/v1"
        model_cfg = {"name": name, "base_url": base, "params": m.get("params", {}), "coords": m.get("coords", "pixel"),
                     "vision": m.get("vision", False), "keep_reasoning": m.get("keep_reasoning", False),
                     "max_tokens": m.get("max_tokens", 6144), "engine": m["engine"]}
        if not args.no_serve:
            subprocess.run(server_cmd() + ["start", name, "--port", str(args.port), "--parallel", str(args.parallel),
                                           "--ctx", str(args.ctx)], check=True)
            bench = subprocess.run(server_cmd() + ["bench", name, "--port", str(args.port), "--concurrency",
                                                   f"1,{args.parallel}"], capture_output=True, text=True)
            (run_dir / f"bench-{name}.json").write_text(bench.stdout)
            print(bench.stdout, flush=True)
        try:
            run_batch(args, drv, run_dir, name + ("+skill" if args.skill else ""), model_cfg, chosen, results)
        finally:
            if not args.no_serve:
                subprocess.run(server_cmd() + ["stop"])


if __name__ == "__main__":
    main()
