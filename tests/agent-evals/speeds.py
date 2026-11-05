"""The decode benchmarks run.py records before each model's episodes
(bench-<model>.json), as a Markdown table.

    python3 speeds.py .work/evals/runs/RUN [RUN…]
"""

import json
import sys
import tomllib
from pathlib import Path

MODELS = tomllib.loads((Path(__file__).resolve().parent / "models.toml").read_text())["models"]


def main():
    rows = {}
    for run in sys.argv[1:]:
        for f in sorted(Path(run).glob("bench-*.json")):
            try:
                rows[f.stem[len("bench-"):]] = json.loads(f.read_text())
            except json.JSONDecodeError:
                pass
    print("| Model | File | Decode, 1 stream | Aggregate, 4 streams | Prefill, ~20K tokens | Next turn, cached |")
    print("|---|---|---:|---:|---:|---:|")
    for name, b in sorted(rows.items(), key=lambda kv: -kv[1]["concurrency_1"]["per_stream_decode_tps"]):
        m = MODELS.get(name, {})
        lc = b["long_context"]
        print(f"| {name} | {m.get('file', m.get('repo', ''))} ({m.get('size_gb', '?')} GB) | "
              f"{b['concurrency_1']['per_stream_decode_tps']:.0f} tok/s | {b['concurrency_4']['aggregate_tps']:.0f} tok/s | "
              f"{lc['cold_prefill_tps']:,} tok/s | {lc['cached_ttft']:.2f} s |")


if __name__ == "__main__":
    main()
